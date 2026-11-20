import { access, realpath } from 'node:fs/promises';
import { homedir } from 'node:os';
import { basename, dirname, join } from 'node:path';
import { CheckerError } from '../checker-error.mts';
import type { Check } from '../types.d.mts';
import { readPythonPaths } from './python-paths.mts';
import { readPythonTool } from './python-tool.mts';
import { readRustTool } from './rust-tool.mts';

const SANDBOX_EXEC = '/usr/bin/sandbox-exec';
const SYSTEM_PATH = '/usr/bin:/bin';

const NODE_FLAGS = [
  '--disable-wasm-trap-handler',
  '--max-old-space-size=96',
  '--v8-pool-size=2',
] as const;

type MacosSandboxInput = Readonly<{
  check: Check;
  files: ReadonlyMap<string, string>;
  probeFile: string;
  probeTmp: string;
  work: string;
}>;

export type SandboxCommand = Readonly<{
  args: readonly string[];
  cwd?: string;
  env: Readonly<Record<string, string>>;
  file: string;
}>;

type ProfileInput = Readonly<{
  probeTmp: string;
  runtimeRoot: string;
  work: string;
}>;

type ProgramInput = Readonly<{
  check: Check;
  probeFile: string;
  probeTmp: string;
}>;

type ResolvedRuntime = Readonly<{
  command: readonly string[];
  runtimeRoot: string;
}>;

function escapeScheme(value: string): string {
  return value.replaceAll('\\', String.raw`\\`).replaceAll('"', String.raw`\"`);
}

function assertBoundedRuntimeRoot(root: string): string {
  const userHome = homedir();
  const forbiddenRoots: ReadonlySet<string> = new Set(['/', '/Users', '/private', '/var', userHome]);

  if (forbiddenRoots.has(root)) {
    throw new CheckerError({ message: 'runtime installation path cannot be safely sandboxed' });
  }

  return root;
}

function readInstallRoot(binary: string): string {
  const binDir = dirname(binary);
  const installRoot = dirname(binDir);

  return assertBoundedRuntimeRoot(installRoot);
}

async function resolveNodeRuntime({ check, probeFile }: ProgramInput): Promise<ResolvedRuntime> {
  const binary = await realpath(process.execPath);
  const runtimeRoot = readInstallRoot(binary);

  if (check.testFile === undefined) {
    return { command: [binary, ...NODE_FLAGS, `./${probeFile}`], runtimeRoot };
  }

  return {
    command: [binary, ...NODE_FLAGS, '--test', '--test-reporter=tap', '--', `./${check.testFile}`],
    runtimeRoot,
  };
}

async function resolvePythonRuntime({ check, probeFile }: ProgramInput): Promise<ResolvedRuntime> {
  const binary = await readPythonTool();
  const runtimeRoot = readInstallRoot(binary);

  if (check.testFile === undefined) {
    return { command: [binary, '-B', `./${probeFile}`], runtimeRoot };
  }

  return {
    command: [
      binary,
      '-B',
      '-m',
      'unittest',
      'discover',
      '-s',
      `./${dirname(check.testFile)}`,
      '-p',
      basename(check.testFile),
    ],
    runtimeRoot,
  };
}

async function resolveRustRuntime({ probeFile, probeTmp }: ProgramInput): Promise<ResolvedRuntime> {
  const sysroot = await readRustTool('--print=sysroot');
  const resolved = await realpath(sysroot);
  const runtimeRoot = assertBoundedRuntimeRoot(resolved);
  const rustc = join(runtimeRoot, 'bin', 'rustc');
  const output = join(probeTmp, 'observe');

  return {
    command: [
      '/bin/sh',
      '-c',
      '"$1" --edition=2021 --crate-name tenet_probe ./"$2" -o "$3" && exec "$3"',
      'tenet-rust',
      rustc,
      probeFile,
      output,
    ],
    runtimeRoot,
  };
}

async function resolveRuntime(input: ProgramInput): Promise<ResolvedRuntime> {
  switch (input.check.runtime) {
    case 'node': {
      return await resolveNodeRuntime(input);
    }
    case 'python': {
      return await resolvePythonRuntime(input);
    }
    case 'rust': {
      return await resolveRustRuntime(input);
    }
  }
}

function buildSeatbeltProfile({ probeTmp, runtimeRoot, work }: ProfileInput): string {
  const safeRuntime = escapeScheme(runtimeRoot);
  const safeWork = escapeScheme(work);
  const safeTmp = escapeScheme(probeTmp);

  return [
    '(version 1)',
    '(deny default)',
    '(allow process-fork process-exec)',
    '(allow signal (target same-sandbox))',
    '(allow process-info* (target same-sandbox))',
    '(allow sysctl-read)',
    '(allow mach-lookup)',
    '(allow file-read*)',
    '(allow file-map-executable)',
    '(deny file-read-data (subpath "/Users") (subpath "/Volumes") (subpath "/private/tmp") (subpath "/private/var/folders"))',
    `(allow file-read-data (subpath "${safeRuntime}") (subpath "${safeWork}") (subpath "${safeTmp}"))`,
    `(allow file-write* (subpath "${safeTmp}") (literal "/dev/null") (literal "/dev/zero"))`,
    '(deny network*)',
  ].join('\n');
}

type SandboxEnvInput = Readonly<{
  files: ReadonlyMap<string, string>;
  probeTmp: string;
  runtime: Check['runtime'];
  work: string;
}>;

function buildSandboxEnv({
  files,
  probeTmp,
  runtime,
  work,
}: SandboxEnvInput): Readonly<Record<string, string>> {
  const commonEnv = {
    HOME: probeTmp,
    OMP_NUM_THREADS: '1',
    OPENBLAS_NUM_THREADS: '1',
    PATH: SYSTEM_PATH,
    PYTHONDONTWRITEBYTECODE: '1',
    RAYON_NUM_THREADS: '2',
    TMPDIR: probeTmp,
    UV_THREADPOOL_SIZE: '2',
  };

  if (runtime !== 'python') {
    return commonEnv;
  }

  const pythonPath = readPythonPaths({ files, root: work });

  return { ...commonEnv, PYTHONPATH: pythonPath };
}

/** Build a macOS Seatbelt sandbox invocation with read-only source, private tmpdir and no network. */
export async function createMacosSandbox({
  check,
  files,
  probeFile,
  probeTmp,
  work,
}: MacosSandboxInput): Promise<SandboxCommand> {
  await access(SANDBOX_EXEC);

  const canonicalWork = await realpath(work);
  const canonicalTmp = await realpath(probeTmp);

  const { command, runtimeRoot } = await resolveRuntime({
    check,
    probeFile,
    probeTmp: canonicalTmp,
  });

  const profile = buildSeatbeltProfile({
    probeTmp: canonicalTmp,
    runtimeRoot,
    work: canonicalWork,
  });

  const env = buildSandboxEnv({
    files,
    probeTmp: canonicalTmp,
    runtime: check.runtime,
    work: canonicalWork,
  });

  return {
    args: [
      '-p',
      profile,
      '/bin/sh',
      '-c',
      'ulimit -t 8 -f 65536 -c 0 && exec "$@"',
      'tenet-limit',
      ...command,
    ],
    cwd: canonicalWork,
    env,
    file: SANDBOX_EXEC,
  };
}
