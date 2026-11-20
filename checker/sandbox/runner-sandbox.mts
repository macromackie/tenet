import { writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { CheckerError } from '../checker-error.mts';
import type { Check } from '../types.d.mts';
import { type Execution, runFile } from './host-tool.mts';
import { createLinuxSandbox } from './linux-sandbox.mts';
import { createMacosSandbox, type SandboxCommand } from './macos-sandbox.mts';

type SandboxInput = Readonly<{
  check: Check;
  files: ReadonlyMap<string, string>;
  probeTmp: string;
  work: string;
}>;

type Probe = Readonly<{ check: Check; command: SandboxCommand }>;

type AssertionInput = Readonly<{ result: Execution; runtime: Check['runtime'] }>;

type FailureInput = Readonly<{ base: string; stderr: string }>;

const PROBE_TIMEOUT_MS = 15_000;
const RUST_PROBE_TIMEOUT_MS = 30_000;
const OUTPUT_BYTES = 65_536;
const DIAGNOSTIC_LINES = 2;
const DIAGNOSTIC_CHARS = 240;
const ASCII_SPACE = 32;
const ASCII_DEL = 127;
const DIAGNOSTIC_FOOTER = /^(?:error: aborting due to |For more information about this error)/v;
const DIAGNOSTIC_ERROR = /^error(?:\[|:)/v;

const PROBE_FILES = {
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

function stripControlChars(text: string): string {
  let clean = '';

  for (const character of text) {
    const code = character.codePointAt(0) ?? 0;

    if (code >= ASCII_SPACE && code !== ASCII_DEL) {
      clean += character;
    } else {
      clean += ' ';
    }
  }

  return clean;
}

function selectDiagnosticLines(lines: readonly string[]): readonly string[] {
  const highlighted = lines.filter((line) => DIAGNOSTIC_ERROR.test(line));

  if (highlighted.length > 0) {
    return highlighted.slice(0, DIAGNOSTIC_LINES);
  }

  return lines.slice(-DIAGNOSTIC_LINES);
}

function summarizeDiagnostic(stderr: string): string {
  const lines = stderr
    .split('\n')
    .map((line) => line.trim())
    .filter((line) => line.length > 0 && !DIAGNOSTIC_FOOTER.test(line));

  const selected = selectDiagnosticLines(lines).join(' | ');
  const summary = stripControlChars(selected).trim();

  if (summary.length <= DIAGNOSTIC_CHARS) {
    return summary;
  }

  return summary.slice(0, DIAGNOSTIC_CHARS);
}

function formatFailure({ base, stderr }: FailureInput): string {
  const detail = summarizeDiagnostic(stderr);

  if (detail.length === 0) {
    return base;
  }

  return `${base}: ${detail}`;
}

async function buildCommand({
  check,
  files,
  probeTmp,
  work,
}: SandboxInput): Promise<SandboxCommand> {
  const entry = probeFile(check.runtime);

  if (process.platform === 'darwin') {
    return await createMacosSandbox({ check, files, probeFile: entry, probeTmp, work });
  }

  if (process.platform === 'linux') {
    return await createLinuxSandbox({ check, files, probeFile: entry, work });
  }

  throw new CheckerError({
    message: 'an isolated Linux or macOS runner is required for behavior checks',
  });
}

function readProbeTimeout(runtime: Check['runtime']): number {
  if (runtime === 'rust') {
    return RUST_PROBE_TIMEOUT_MS;
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

  const message = formatFailure({
    base: 'the selected native test produced no clear assertion result',
    stderr: result.stderr,
  });

  throw new CheckerError({ message });
}

function assertCompleted(result: Execution): void {
  if (result.status !== 0 && result.stderr.startsWith('bwrap: Unknown option --size')) {
    throw new CheckerError({
      message: 'install bubblewrap 0.7 or later to enforce the 64 MiB temporary filesystem limit',
    });
  }

  if (result.status !== 0) {
    const code = String(result.status);

    const message = formatFailure({
      base: `the isolated test exited with status ${code}`,
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

  assertCompleted(result);

  const observed: unknown = JSON.parse(result.stdout);

  return observed;
}

/** Stage the test and return an isolated observation that can be replayed against each control. */
export async function createSandbox(input: SandboxInput): Promise<() => Promise<unknown>> {
  const { check, work } = input;

  if (check.test !== undefined) {
    const entry = probeFile(check.runtime);
    const targetPath = join(work, entry);

    await writeFile(targetPath, check.test, { flag: 'wx' });
  }

  const command = await buildCommand(input);

  return async () => await runProbe({ check, command });
}
