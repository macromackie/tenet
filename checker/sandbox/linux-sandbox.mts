import { access, realpath } from 'node:fs/promises';
import { basename, dirname } from 'node:path';
import type { Check } from '../types.d.mts';
import type { SandboxCommand } from './macos-sandbox.mts';
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
  '/toolchain/bin/rustc --edition=2021 --crate-name tenet_probe -C linker=/usr/bin/gcc .tenet-observe.rs -o /tmp/observe && exec /tmp/observe',
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

type ProgramInput = Readonly<{ check: Check; probeFile: string }>;

type EnvFlagsInput = Readonly<{
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

function buildProgram({ check, probeFile }: ProgramInput): readonly string[] {
  switch (check.runtime) {
    case 'node': {
      return buildNodeProgram({ check, probeFile });
    }
    case 'python': {
      return buildPythonProgram({ check, probeFile });
    }
    case 'rust': {
      return RUST_COMMAND;
    }
  }
}

function buildEnvFlags({ files, runtime }: EnvFlagsInput): readonly string[] {
  if (runtime !== 'python') {
    return BASE_ENV_FLAGS;
  }

  const pythonPath = readPythonPaths({ files, root: '/project' });

  return [...BASE_ENV_FLAGS, '--setenv', 'PYTHONPATH', pythonPath];
}

/** Build a Linux bubblewrap + prlimit sandbox invocation for an isolated behavior probe. */
export async function createLinuxSandbox({
  check,
  files,
  probeFile,
  work,
}: LinuxSandboxInput): Promise<SandboxCommand> {
  await access(BWRAP);
  await access(PRLIMIT);

  const mounts = await buildRuntimeMount(check.runtime);
  const envFlags = buildEnvFlags({ files, runtime: check.runtime });
  const program = buildProgram({ check, probeFile });

  return {
    args: [
      ...ISOLATION,
      ...mounts,
      '--ro-bind',
      work,
      '/project',
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
