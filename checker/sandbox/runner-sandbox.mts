import { mkdir, symlink, writeFile } from 'node:fs/promises';
import { dirname, join } from 'node:path';
import { CheckerError } from '../checker-error.mts';
import type { Check } from '../types.d.mts';
import { formatProbeFailure, parseProbeStdout } from './controls/index.mts';
import { type Execution, runFile } from './host-tool.mts';
import { createLinuxSandbox, createMacosSandbox, type SandboxCommand } from './os/index.mts';
import { type ProjectDeps, resolveProjectDeps } from './project-deps.mts';

type SandboxInput = Readonly<{
  check: Check;
  files: ReadonlyMap<string, string>;
  probeTmp: string;
  projectRoot?: string;
  work: string;
}>;

type Probe = Readonly<{
  check: Check;
  command: SandboxCommand;
  deps: ProjectDeps;
  filePaths: readonly string[];
}>;

type AssertionInput = Readonly<{ result: Execution; runtime: Check['runtime'] }>;

type DepStageInput = Readonly<{
  deps: ProjectDeps;
  files: ReadonlyMap<string, string>;
  work: string;
}>;

type StageLinkInput = Readonly<{ source?: string; target: string }>;

type AssertCompletedInput = Readonly<{
  deps: ProjectDeps;
  filePaths: readonly string[];
  result: Execution;
  runtime: Check['runtime'];
}>;

type BuildCommandInput = SandboxInput & Readonly<{ deps: ProjectDeps }>;

type GoModFallbackInput = Readonly<{
  check: Check;
  deps: ProjectDeps;
  files: ReadonlyMap<string, string>;
  work: string;
}>;

type WriteProbeInput = Readonly<{ check: Check; work: string }>;

const PROBE_TIMEOUT_MS = 15_000;
const COMPILED_PROBE_TIMEOUT_MS = 30_000;
const OUTPUT_BYTES = 65_536;
const COMPILED_RUNTIMES: ReadonlySet<string> = new Set(['go', 'rust']);

const PROBE_FILES = {
  go: '.tenet_observe/main.go',
  node: '.tenet-observe.mjs',
  python: '.tenet-observe.py',
  rust: '.tenet-observe.rs',
} as const;

type Markers = Readonly<{ failed: RegExp; passed: RegExp }>;

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

function probeFile(runtime: Check['runtime']): string {
  return PROBE_FILES[runtime];
}

async function createDepLink({ source, target }: StageLinkInput): Promise<void> {
  if (source === undefined) {
    return;
  }

  if (process.platform === 'darwin') {
    await symlink(source, target, 'dir');

    return;
  }

  await mkdir(target, { recursive: true });
}

async function prepareDepLinks({ deps, files, work }: DepStageInput): Promise<void> {
  const nodeTarget = join(work, 'node_modules');

  await createDepLink({ source: deps.nodeModules, target: nodeTarget });

  const hasStagedVendor = [...files.keys()].some((path) => path.startsWith('vendor/'));

  if (!hasStagedVendor) {
    const vendorTarget = join(work, 'vendor');

    await createDepLink({ source: deps.goVendor, target: vendorTarget });
  }
}

async function buildCommand({
  check,
  deps,
  files,
  probeTmp,
  projectRoot,
  work,
}: BuildCommandInput): Promise<SandboxCommand> {
  const entry = probeFile(check.runtime);

  if (process.platform === 'darwin') {
    return await createMacosSandbox({
      check,
      deps,
      files,
      probeFile: entry,
      probeTmp,
      projectRoot,
      work,
    });
  }

  if (process.platform === 'linux') {
    return await createLinuxSandbox({ check, deps, files, probeFile: entry, projectRoot, work });
  }

  throw new CheckerError({
    message: 'an isolated Linux or macOS runner is required for behavior checks',
  });
}

function readProbeTimeout(runtime: Check['runtime']): number {
  if (COMPILED_RUNTIMES.has(runtime)) {
    return COMPILED_PROBE_TIMEOUT_MS;
  }

  return PROBE_TIMEOUT_MS;
}

async function execute(probe: Probe): Promise<Execution> {
  const { args, cwd, env, file } = probe.command;
  const timeout = readProbeTimeout(probe.check.runtime);

  return await runFile({
    args,
    file,
    options: {
      cwd,
      encoding: 'utf-8',
      env,
      killSignal: 'SIGKILL',
      maxBuffer: OUTPUT_BYTES,
      timeout,
    },
  });
}

function isPassing({ result, runtime }: AssertionInput): boolean {
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

  const message = formatProbeFailure({
    base: 'the selected native test produced no clear assertion result',
    stderr: result.stderr,
  });

  throw new CheckerError({ message });
}

function assertCompleted({ deps, filePaths, result, runtime }: AssertCompletedInput): void {
  if (result.status !== 0 && result.stderr.startsWith('bwrap: Unknown option --size')) {
    throw new CheckerError({
      message: 'install bubblewrap 0.7 or later to enforce the 64 MiB temporary filesystem limit',
    });
  }

  if (result.status !== 0) {
    const code = String(result.status);

    const message = formatProbeFailure({
      base: `the isolated test exited with status ${code}`,
      deps,
      filePaths,
      runtime,
      stderr: result.stderr,
    });

    throw new CheckerError({ message });
  }
}

async function runProbe(probe: Probe): Promise<unknown> {
  const result = await execute(probe);

  if (probe.check.testFile !== undefined) {
    if (result.status !== 0 && result.stderr.startsWith('bwrap: Unknown option --size')) {
      throw new CheckerError({
        message: 'install bubblewrap 0.7 or later to enforce the 64 MiB temporary filesystem limit',
      });
    }

    return isPassing({ result, runtime: probe.check.runtime });
  }

  assertCompleted({
    deps: probe.deps,
    filePaths: probe.filePaths,
    result,
    runtime: probe.check.runtime,
  });

  return parseProbeStdout(result.stdout);
}

async function createGoModFallback({
  check,
  deps,
  files,
  work,
}: GoModFallbackInput): Promise<void> {
  if (check.runtime !== 'go') {
    return;
  }

  if (!files.has('go.mod')) {
    const goModPath = join(work, 'go.mod');
    const goModContent = deps.goMod ?? 'module tenet_check\n\ngo 1.21\n';

    await writeFile(goModPath, goModContent, { flag: 'wx' });
  }

  if (!files.has('go.sum') && deps.goSum !== undefined) {
    const goSumPath = join(work, 'go.sum');

    await writeFile(goSumPath, deps.goSum, { flag: 'wx' });
  }
}

async function writeProbeFile({ check, work }: WriteProbeInput): Promise<void> {
  if (check.test === undefined) {
    return;
  }

  const entry = probeFile(check.runtime);
  const targetPath = join(work, entry);
  const parentDir = dirname(targetPath);

  await mkdir(parentDir, { recursive: true });
  await writeFile(targetPath, check.test, { flag: 'wx' });
}

/** Stage the test and return an isolated observation that can be replayed against each control. */
export async function createSandbox(input: SandboxInput): Promise<() => Promise<unknown>> {
  const { check, files, projectRoot, work } = input;
  const deps = await resolveProjectDeps({ projectRoot, runtime: check.runtime });

  await prepareDepLinks({ deps, files, work });
  await createGoModFallback({ check, deps, files, work });
  await writeProbeFile({ check, work });

  const command = await buildCommand({ ...input, deps });
  const filePaths = [...files.keys()];

  return async () => await runProbe({ check, command, deps, filePaths });
}
