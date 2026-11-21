import { access, realpath } from 'node:fs/promises';
import { dirname } from 'node:path';
import type { Check } from '../../types.d.mts';
import type { ProjectDeps } from '../project-deps.mts';
import {
  buildGoCommand,
  buildGoEnv,
  buildNodeCommand,
  buildPythonCommand,
  buildRustCommand,
  readGoTool,
  readPythonPaths,
  readRustCrate,
  readRustTool,
} from '../runtimes/index.mts';
import type { SandboxCommand } from './macos-sandbox.mts';

const BWRAP = '/usr/bin/bwrap';
const PYTHON = '/usr/bin/python3';
const PRLIMIT = '/usr/bin/prlimit';
const SYSTEM_PATH = '/usr/bin:/bin';
const NODE = '/runtime/node';
const GO_ROOT_MOUNT = '/goroot';
const COMPILED_RUNTIMES: ReadonlySet<string> = new Set(['go', 'rust']);

const ISOLATION = [
  ['--unshare-all', '--new-session', '--die-with-parent', '--clearenv'],
  ['--cap-drop', 'ALL'],
  ['--dir', '/usr'],
  ['--ro-bind', '/usr/bin', '/usr/bin'],
  ['--ro-bind', '/usr/lib', '/usr/lib'],
  ['--ro-bind-try', '/usr/lib64', '/usr/lib64'],
  ['--symlink', 'usr/bin', '/bin'],
  ['--symlink', 'usr/lib', '/lib'],
  ['--symlink', 'usr/lib64', '/lib64'],
  ['--proc', '/proc'],
  ['--dev', '/dev'],
  ['--size', '67108864', '--tmpfs', '/tmp'],
  ['--dir', '/runtime'],
  ['--dir', '/deps'],
].flat();

const BASE_ENV_FLAGS = [
  ['--setenv', 'PATH', SYSTEM_PATH],
  ['--setenv', 'HOME', '/tmp'],
  ['--setenv', 'TMPDIR', '/tmp'],
  ['--setenv', 'UV_THREADPOOL_SIZE', '2'],
  ['--setenv', 'OMP_NUM_THREADS', '1'],
  ['--setenv', 'OPENBLAS_NUM_THREADS', '1'],
  ['--setenv', 'RAYON_NUM_THREADS', '2'],
  ['--setenv', 'PYTHONDONTWRITEBYTECODE', '1'],
].flat();

type LinuxSandboxInput = Readonly<{
  check: Check;
  deps: ProjectDeps;
  files: ReadonlyMap<string, string>;
  probeFile: string;
  projectRoot?: string;
  work: string;
}>;

type ProgramInput = Readonly<{
  check: Check;
  deps: ProjectDeps;
  files: ReadonlyMap<string, string>;
  probeFile: string;
  projectRoot?: string;
}>;

type EnvFlagsInput = Readonly<{
  deps: ProjectDeps;
  files: ReadonlyMap<string, string>;
  runtime: Check['runtime'];
}>;

function readCpuFlag(runtime: Check['runtime']): string {
  if (COMPILED_RUNTIMES.has(runtime)) {
    return '--cpu=20';
  }

  return '--cpu=8';
}

function buildLimits(runtime: Check['runtime']): readonly string[] {
  const cpu = readCpuFlag(runtime);

  return [
    PRLIMIT,
    cpu,
    '--as=4294967296',
    '--nproc=512',
    '--fsize=33554432',
    '--nofile=256',
    '--core=0',
    '--',
  ];
}

async function buildRuntimeMount(runtime: Check['runtime']): Promise<readonly string[]> {
  switch (runtime) {
    case 'go': {
      const rawGoroot = await readGoTool(['env', 'GOROOT']);
      const goroot = await realpath(rawGoroot);

      return ['--ro-bind', goroot, GO_ROOT_MOUNT];
    }
    case 'node': {
      return ['--ro-bind', await realpath(process.execPath), NODE];
    }
    case 'python': {
      return [];
    }
    case 'rust': {
      const sysroot = await readRustTool('--print=sysroot');
      const toolchain = await realpath(sysroot);

      return [
        '--ro-bind',
        toolchain,
        '/toolchain',
        '--ro-bind-try',
        '/usr/libexec/gcc',
        '/usr/libexec/gcc',
      ];
    }
  }
}

function buildDepMounts(deps: ProjectDeps): readonly string[] {
  const mounts: string[] = [];

  if (deps.goVendor !== undefined) {
    mounts.push('--ro-bind', deps.goVendor, '/project/vendor');
  }

  if (deps.nodeModules !== undefined) {
    mounts.push('--ro-bind', deps.nodeModules, '/project/node_modules');
  }

  if (deps.pythonSitePackages !== undefined) {
    mounts.push('--ro-bind', deps.pythonSitePackages, '/deps/python');
  }

  if (deps.rustDepsDir !== undefined) {
    mounts.push('--ro-bind', deps.rustDepsDir, '/deps/rust');
  }

  return mounts;
}

function buildGoProgram({ deps, files, probeFile }: ProgramInput): readonly string[] {
  const probeDir = dirname(probeFile);

  return buildGoCommand({
    binary: `${GO_ROOT_MOUNT}/bin/go`,
    deps,
    files,
    output: '/tmp/observe',
    probeDir,
  });
}

async function buildProgram({
  check,
  deps,
  files,
  probeFile,
  projectRoot,
}: ProgramInput): Promise<readonly string[]> {
  switch (check.runtime) {
    case 'go': {
      return buildGoProgram({ check, deps, files, probeFile, projectRoot });
    }
    case 'node': {
      return buildNodeCommand({ binary: NODE, check, probeFile });
    }
    case 'python': {
      return buildPythonCommand({ binary: PYTHON, check, probeFile });
    }
    case 'rust': {
      const rustCrate = await readRustCrate({ files, projectRoot });

      return buildRustCommand({
        deps,
        depsMount: '/deps/rust',
        linkerArg: '/usr/bin/gcc',
        probeFile,
        probeTmp: '/tmp',
        rustCrate,
        rustc: '/toolchain/bin/rustc',
      });
    }
  }
}

function buildEnvFlags({ deps, files, runtime }: EnvFlagsInput): readonly string[] {
  if (runtime === 'go') {
    const goEnv = buildGoEnv({ goroot: GO_ROOT_MOUNT, probeTmp: '/tmp' });
    const goFlags = Object.entries(goEnv).flatMap(([key, value]) => ['--setenv', key, value]);

    return [...BASE_ENV_FLAGS, ...goFlags];
  }

  if (runtime !== 'python') {
    return BASE_ENV_FLAGS;
  }

  const basePath = readPythonPaths({ files, root: '/project' });

  if (deps.pythonSitePackages === undefined) {
    return [...BASE_ENV_FLAGS, '--setenv', 'PYTHONPATH', basePath];
  }

  return [...BASE_ENV_FLAGS, '--setenv', 'PYTHONPATH', `${basePath}:/deps/python`];
}

/** Build a Linux bubblewrap + prlimit sandbox invocation for an isolated behavior probe. */
export async function createLinuxSandbox({
  check,
  deps,
  files,
  probeFile,
  projectRoot,
  work,
}: LinuxSandboxInput): Promise<SandboxCommand> {
  await access(BWRAP);
  await access(PRLIMIT);

  const mounts = await buildRuntimeMount(check.runtime);
  const depMounts = buildDepMounts(deps);
  const envFlags = buildEnvFlags({ deps, files, runtime: check.runtime });
  const program = await buildProgram({ check, deps, files, probeFile, projectRoot });
  const limits = buildLimits(check.runtime);

  return {
    args: [
      ...ISOLATION,
      ...mounts,
      '--ro-bind',
      work,
      '/project',
      ...depMounts,
      '--chdir',
      '/project',
      ...envFlags,
      '--',
      ...limits,
      ...program,
    ],
    env: { PATH: SYSTEM_PATH },
    file: BWRAP,
  };
}
