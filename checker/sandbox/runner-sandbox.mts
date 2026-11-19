import { access, realpath, writeFile } from 'node:fs/promises';
import { basename, dirname, join } from 'node:path';
import { CheckerError } from '../checker-error.mts';
import type { Check } from '../types.d.mts';
import { type Execution, runFile } from './host-tool.mts';
import { readRustTool } from './rust-tool.mts';

type MountInput = Readonly<{ work: string; runtime: Check['runtime'] }>;
type SandboxInput = Readonly<{ work: string; check: Check }>;
type Probe = Readonly<{ check: Check; mounts: readonly string[] }>;
type AssertionInput = Readonly<{ runtime: Check['runtime']; result: Execution }>;
const BWRAP = '/usr/bin/bwrap';
const PYTHON = '/usr/bin/python3';
const PRLIMIT = '/usr/bin/prlimit';
const SYSTEM_PATH = '/usr/bin:/bin';
const PROBE_TIMEOUT_MS = 11_000;
const OUTPUT_BYTES = 65_536;
const NODE = '/runtime/node';

const PROBE_FILES = {
  node: '.tenet-observe.mjs',
  python: '.tenet-observe.py',
  rust: '.tenet-observe.rs',
} as const;

const NODE_FLAGS = ['--disable-wasm-trap-handler', '--max-old-space-size=96'] as const;

const LIMITS = [
  PRLIMIT,
  '--cpu=8',
  '--as=1600000000',
  '--nproc=64',
  '--fsize=33554432',
  '--nofile=128',
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

function probeFile(runtime: Check['runtime']): string {
  return PROBE_FILES[runtime];
}

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

async function buildSandboxMounts({ work, runtime }: MountInput): Promise<readonly string[]> {
  if (process.platform !== 'linux') {
    throw new CheckerError({
      message: 'an isolated Linux runner with bubblewrap and prlimit is required',
    });
  }

  await access(BWRAP);
  await access(PRLIMIT);

  const mounts = await buildRuntimeMount(runtime);

  return [
    ...ISOLATION,
    ...mounts,
    '--ro-bind',
    work,
    '/project',
    '--chdir',
    '/project',
    '--setenv',
    'PATH',
    SYSTEM_PATH,
    '--setenv',
    'HOME',
    '/tmp',
    '--setenv',
    'TMPDIR',
    '/tmp',
  ];
}

function buildProgram(check: Check): readonly string[] {
  switch (check.runtime) {
    case 'node': {
      if (check.testFile !== undefined) {
        return [NODE, ...NODE_FLAGS, '--test', '--test-reporter=tap', '--', `./${check.testFile}`];
      }

      return [NODE, ...NODE_FLAGS, `./${probeFile(check.runtime)}`];
    }
    case 'python': {
      if (check.testFile !== undefined) {
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

      return [PYTHON, '-B', `./${probeFile(check.runtime)}`];
    }
    case 'rust': {
      return RUST_COMMAND;
    }
  }
}

async function execute(probe: Probe): Promise<Execution> {
  const command = buildProgram(probe.check);

  return await runFile({
    args: [...probe.mounts, '--', ...LIMITS, ...command],
    file: BWRAP,
    options: {
      encoding: 'utf-8',
      env: { PATH: SYSTEM_PATH },
      killSignal: 'SIGKILL',
      maxBuffer: OUTPUT_BYTES,
      timeout: PROBE_TIMEOUT_MS,
    },
  });
}

type Markers = Readonly<{ passed: RegExp; failed: RegExp }>;

const ASSERTION_MARKERS: ReadonlyMap<string, Markers> = new Map([
  [
    'node',
    {
      failed: /^\s*code: 'ERR_ASSERTION'$/mv,
      passed: /^# pass [1-9]\d*$/mv,
    },
  ],
  [
    'python',
    {
      failed: /^FAILED \(failures=[1-9]\d*\)$/mv,
      passed: /^Ran [1-9]\d* tests? in [^\n]+\n\s*OK\s*$/mv,
    },
  ],
]);

function isPassing({ runtime, result }: AssertionInput): boolean {
  const logs = [result.stdout, result.stderr].join('\n');

  if (runtime === 'node' && /^# (?:skipped|todo) [1-9]/mv.test(logs)) {
    throw new CheckerError({ message: 'the selected native test skipped an observation' });
  }

  const markers = ASSERTION_MARKERS.get(runtime);

  if (markers !== undefined && result.status === 0 && markers.passed.test(logs)) {
    return true;
  }

  if (markers !== undefined && result.status === 1 && markers.failed.test(logs)) {
    return false;
  }

  throw new CheckerError({
    message: 'the selected native test produced no clear assertion result',
  });
}

async function runProbe(probe: Probe): Promise<unknown> {
  const result = await execute(probe);

  if (result.status !== 0 && result.stderr.startsWith('bwrap: Unknown option --size')) {
    throw new CheckerError({
      message: 'install bubblewrap 0.7 or later to enforce the 64 MiB temporary filesystem limit',
    });
  }

  if (probe.check.testFile !== undefined) {
    return isPassing({ result, runtime: probe.check.runtime });
  }

  if (result.status !== 0) {
    throw new CheckerError({ message: 'the isolated test could not run to completion' });
  }

  const observed: unknown = JSON.parse(result.stdout);

  return observed;
}

/** Stage the test and return an isolated observation that can be replayed against each control. */
export async function createSandbox({
  work,
  check,
}: SandboxInput): Promise<() => Promise<unknown>> {
  if (check.test !== undefined) {
    const entry = probeFile(check.runtime);

    const targetPath = join(work, entry);

    await writeFile(targetPath, check.test, { flag: 'wx' });
  }

  const mounts = await buildSandboxMounts({ runtime: check.runtime, work });

  return async () => await runProbe({ check, mounts });
}
