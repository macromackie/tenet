import { access, realpath } from 'node:fs/promises';
import { homedir } from 'node:os';
import { dirname, join } from 'node:path';
import { CheckerError } from '../../checker-error.mts';
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
  readPythonTool,
  readRustCrate,
  readRustTool,
} from '../runtimes/index.mts';
import { buildSeatbeltProfile } from './macos-profile.mts';

const SANDBOX_EXEC = '/usr/bin/sandbox-exec';
const SYSTEM_PATH = '/usr/bin:/bin';
const DEFAULT_CPU_LIMIT = '8';
const COMPILED_CPU_LIMIT = '20';
const COMPILED_RUNTIMES: ReadonlySet<string> = new Set(['go', 'rust']);

type MacosSandboxInput = Readonly<{
  check: Check;
  deps: ProjectDeps;
  files: ReadonlyMap<string, string>;
  probeFile: string;
  probeTmp: string;
  projectRoot?: string;
  work: string;
}>;

export type SandboxCommand = Readonly<{
  args: readonly string[];
  cwd?: string;
  env: Readonly<Record<string, string>>;
  file: string;
}>;

type ProgramInput = Readonly<{
  check: Check;
  deps: ProjectDeps;
  files: ReadonlyMap<string, string>;
  probeFile: string;
  probeTmp: string;
  projectRoot?: string;
}>;

type ResolvedRuntime = Readonly<{
  command: readonly string[];
  extraEnv?: Readonly<Record<string, string>>;
  runtimeRoot: string;
}>;

type SandboxEnvInput = Readonly<{
  deps: ProjectDeps;
  extraEnv?: Readonly<Record<string, string>>;
  files: ReadonlyMap<string, string>;
  probeTmp: string;
  runtime: Check['runtime'];
  work: string;
}>;

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
  const command = buildNodeCommand({ binary, check, probeFile });

  return { command, runtimeRoot };
}

async function resolvePythonRuntime({ check, probeFile }: ProgramInput): Promise<ResolvedRuntime> {
  const binary = await readPythonTool();
  const runtimeRoot = readInstallRoot(binary);
  const command = buildPythonCommand({ binary, check, probeFile });

  return { command, runtimeRoot };
}

async function resolveRustRuntime({
  deps,
  files,
  probeFile,
  probeTmp,
  projectRoot,
}: ProgramInput): Promise<ResolvedRuntime> {
  const sysroot = await readRustTool('--print=sysroot');
  const resolved = await realpath(sysroot);
  const runtimeRoot = assertBoundedRuntimeRoot(resolved);
  const rustc = join(runtimeRoot, 'bin', 'rustc');
  const rustCrate = await readRustCrate({ deps, files, projectRoot });
  const depsMount = deps.rustDepsDir ?? '';
  const command = buildRustCommand({ deps, depsMount, probeFile, probeTmp, rustCrate, rustc });

  return { command, runtimeRoot };
}

async function resolveGoRuntime({
  deps,
  files,
  probeFile,
  probeTmp,
}: ProgramInput): Promise<ResolvedRuntime> {
  const rawGoroot = await readGoTool(['env', 'GOROOT']);
  const canonicalGoroot = await realpath(rawGoroot);
  const runtimeRoot = assertBoundedRuntimeRoot(canonicalGoroot);
  const binary = join(runtimeRoot, 'bin', 'go');
  const probeDir = dirname(probeFile);
  const output = join(probeTmp, 'observe');
  const command = buildGoCommand({ binary, deps, files, output, probeDir });
  const extraEnv = buildGoEnv({ goroot: runtimeRoot, probeTmp });

  return { command, extraEnv, runtimeRoot };
}

async function resolveRuntime(input: ProgramInput): Promise<ResolvedRuntime> {
  switch (input.check.runtime) {
    case 'go': {
      return await resolveGoRuntime(input);
    }
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

function buildSandboxEnv({
  deps,
  extraEnv,
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
    ...extraEnv,
  };

  if (runtime !== 'python') {
    return commonEnv;
  }

  const basePath = readPythonPaths({ files, root: work });

  if (deps.pythonSitePackages === undefined) {
    return { ...commonEnv, PYTHONPATH: basePath };
  }

  return { ...commonEnv, PYTHONPATH: `${basePath}:${deps.pythonSitePackages}` };
}

function readCpuLimit(runtime: Check['runtime']): string {
  if (COMPILED_RUNTIMES.has(runtime)) {
    return COMPILED_CPU_LIMIT;
  }

  return DEFAULT_CPU_LIMIT;
}

/** Build a macOS Seatbelt sandbox invocation with read-only source, private tmpdir and no network. */
export async function createMacosSandbox({
  check,
  deps,
  files,
  probeFile,
  probeTmp,
  projectRoot,
  work,
}: MacosSandboxInput): Promise<SandboxCommand> {
  await access(SANDBOX_EXEC);

  const canonicalWork = await realpath(work);
  const canonicalTmp = await realpath(probeTmp);

  const { command, extraEnv, runtimeRoot } = await resolveRuntime({
    check,
    deps,
    files,
    probeFile,
    probeTmp: canonicalTmp,
    projectRoot,
  });

  const profile = buildSeatbeltProfile({
    deps,
    probeTmp: canonicalTmp,
    runtimeRoot,
    work: canonicalWork,
  });

  const env = buildSandboxEnv({
    deps,
    extraEnv,
    files,
    probeTmp: canonicalTmp,
    runtime: check.runtime,
    work: canonicalWork,
  });

  const cpuLimit = readCpuLimit(check.runtime);
  const limitScript = `ulimit -t ${cpuLimit} -f 65536 -c 0 && exec "$@"`;

  return {
    args: ['-p', profile, '/bin/sh', '-c', limitScript, 'tenet-limit', ...command],
    cwd: canonicalWork,
    env,
    file: SANDBOX_EXEC,
  };
}
