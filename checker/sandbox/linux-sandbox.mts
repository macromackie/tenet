import { access, realpath } from 'node:fs/promises';
import { basename, dirname } from 'node:path';
import type { Check } from '../types.d.mts';
import type { SandboxCommand } from './macos-sandbox.mts';
import type { ProjectDeps } from './project-deps.mts';
import { readPythonPaths } from './python-paths.mts';
import { readRustTool } from './rust-tool.mts';

const BWRAP = '/usr/bin/bwrap';
const PYTHON = '/usr/bin/python3';
const PRLIMIT = '/usr/bin/prlimit';
const SYSTEM_PATH = '/usr/bin:/bin';
const NODE = '/runtime/node';

const NODE_FLAGS = [
  '--disable-wasm-trap-handler',
  '--max-old-space-size=96',
  '--v8-pool-size=2',
] as const;

const LIMITS = [
  PRLIMIT,
  '--cpu=8',
  '--as=4294967296',
  '--nproc=512',
  '--fsize=33554432',
  '--nofile=256',
  '--core=0',
  '--',
] as const;

/** Compilation and execution share the sandbox's private temporary directory. */
const RUST_COMMAND = [
  '/bin/sh',
  '-c',
  '/toolchain/bin/rustc --edition=2021 --crate-name tenet_probe -C linker=/usr/bin/gcc "$@" .tenet-observe.rs -o /tmp/observe && exec /tmp/observe',
  'tenet-rust',
] as const;

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
  work: string;
}>;

async function buildRuntimeMount(runtime: Check['runtime']): Promise<readonly string[]> {
  switch (runtime) {
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

type ProgramInput = Readonly<{ check: Check; deps: ProjectDeps; probeFile: string }>;

type EnvFlagsInput = Readonly<{
  deps: ProjectDeps;
  files: ReadonlyMap<string, string>;
  runtime: Check['runtime'];
}>;

function buildPythonProgram({ check, probeFile }: ProgramInput): readonly string[] {
  if (check.testFile === undefined) {
    return [PYTHON, '-B', `./${probeFile}`];
  }

  return [
    PYTHON,
    '-B',
    '-m',
    'unittest',
    'discover',
    '-s',
    `./${dirname(check.testFile)}`,
    '-p',
    basename(check.testFile),
  ];
}

function buildNodeProgram({ check, probeFile }: ProgramInput): readonly string[] {
  if (check.testFile === undefined) {
    return [NODE, ...NODE_FLAGS, `./${probeFile}`];
  }

  return [NODE, ...NODE_FLAGS, '--test', '--test-reporter=tap', '--', `./${check.testFile}`];
}

function buildRustProgram(deps: ProjectDeps): readonly string[] {
  if (deps.rustDepsDir === undefined) {
    return RUST_COMMAND;
  }

  const externArgs = deps.rustExterns.flatMap(({ file, name }) => [
    '--extern',
    `${name}=/deps/rust/${file}`,
  ]);

  return [...RUST_COMMAND, '-L', 'dependency=/deps/rust', ...externArgs];
}

function buildProgram({ check, deps, probeFile }: ProgramInput): readonly string[] {
  switch (check.runtime) {
    case 'node': {
      return buildNodeProgram({ check, deps, probeFile });
    }
    case 'python': {
      return buildPythonProgram({ check, deps, probeFile });
    }
    case 'rust': {
      return buildRustProgram(deps);
    }
  }
}

function buildEnvFlags({ deps, files, runtime }: EnvFlagsInput): readonly string[] {
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
  work,
}: LinuxSandboxInput): Promise<SandboxCommand> {
  await access(BWRAP);
  await access(PRLIMIT);

  const mounts = await buildRuntimeMount(check.runtime);
  const depMounts = buildDepMounts(deps);
  const envFlags = buildEnvFlags({ deps, files, runtime: check.runtime });
  const program = buildProgram({ check, deps, probeFile });

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
      ...LIMITS,
      ...program,
    ],
    env: { PATH: SYSTEM_PATH },
    file: BWRAP,
  };
}
