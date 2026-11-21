import { mkdir, mkdtemp, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { dirname, join, resolve, sep } from 'node:path';
import { isDeepStrictEqual } from 'node:util';
import { Effect } from 'effect';
import { CheckerError } from '../checker-error.mts';
import type { Check } from '../types.d.mts';
import { createSandbox } from './runner-sandbox.mts';

type ObserveInput = Readonly<{
  check: Check;
  files: ReadonlyMap<string, string>;
  projectRoot?: string;
  qualify: boolean;
  skipActual?: boolean;
}>;

type Project = Omit<ObserveInput, 'qualify' | 'skipActual'> &
  Readonly<{ probeTmp: string; work: string }>;

type Staged = Project & Readonly<{ run: () => Promise<unknown> }>;

type Case = 'good' | 'defect' | 'refactor' | 'actual';

type SampleInput = Readonly<{ project: Staged; variant: Case }>;

type Observation = Readonly<{ passed: boolean; value: unknown }>;

type SourceInput = Readonly<{ path: string; source: string; work: string }>;

type ExecuteInput = Readonly<{
  check: Check;
  project: Staged;
  qualify: boolean;
  skipActual: boolean;
}>;

async function copySource({ path, source, work }: SourceInput): Promise<void> {
  const file = resolve(work, path);
  const prefix = join(work, sep);

  if (!file.startsWith(prefix)) {
    throw new CheckerError({ message: 'a staged source path falls outside the checkout' });
  }

  const parent = dirname(file);

  await mkdir(parent, { recursive: true });
  await writeFile(file, source, { flag: 'wx' });
}

async function prepareProject(project: Project): Promise<Staged> {
  const copies = Effect.forEach(
    project.files,
    ([path, source]) =>
      Effect.tryPromise(async () => await copySource({ path, source, work: project.work })),
    { concurrency: 16 },
  );

  await Effect.runPromise(copies);

  const run = await createSandbox({
    check: project.check,
    files: project.files,
    probeTmp: project.probeTmp,
    projectRoot: project.projectRoot,
    work: project.work,
  });

  return { ...project, run };
}

async function restoreSubject({ project, variant }: SampleInput): Promise<void> {
  const { check, files, work } = project;
  const sources = { ...check, actual: files.get(check.subject) };
  const source = sources[variant];

  if (source === undefined) {
    throw new CheckerError({ message: 'the checker subject disappeared' });
  }

  const targetPath = join(work, check.subject);

  await writeFile(targetPath, source);
}

function readObservationHint(variant: Case): string {
  if (variant === 'refactor') {
    return ' (good and defect already passed; fix only the refactor control)';
  }

  return '';
}

async function readObservation({ project, variant }: SampleInput): Promise<Observation> {
  await restoreSubject({ project, variant });

  const { check } = project;
  const suffix = readObservationHint(variant);

  function createObservationError(cause: unknown): CheckerError {
    const name = JSON.stringify(variant);

    if (cause instanceof Error) {
      const detail = `could not observe ${name} variant: ${cause.message}${suffix}`;

      return new CheckerError({ cause, message: detail });
    }

    const message = `could not observe ${name} variant${suffix}`;

    return new CheckerError({ cause, message });
  }

  const attempt = Effect.tryPromise({ catch: createObservationError, try: project.run });
  const value = await Effect.runPromise(attempt);

  return { passed: isDeepStrictEqual(value, check.expected), value };
}

function readQualificationHint(variant: Case): string {
  if (variant === 'good') {
    return '; if the unmodified subject already violates the contract, set defect to [] and put the fix in good and refactor';
  }

  if (variant === 'refactor') {
    return '; good and defect already passed — keep them and rewrite only refactor as a distinct behavior-preserving variant (for example, extract a helper or invert the condition)';
  }

  return '';
}

async function qualifyControl({ project, variant }: SampleInput): Promise<void> {
  const result = await readObservation({ project, variant });

  if (result.passed !== (variant !== 'defect')) {
    const name = JSON.stringify(variant);
    const observed = JSON.stringify(result.value);
    const expected = JSON.stringify(project.check.expected);
    const suffix = readQualificationHint(variant);
    const message = `test did not distinguish its ${name} control (expected ${expected}, observed ${observed})${suffix}`;

    throw new CheckerError({ message });
  }
}

async function verifyControls(project: Staged): Promise<void> {
  await qualifyControl({ project, variant: 'good' });
  await qualifyControl({ project, variant: 'defect' });
  await qualifyControl({ project, variant: 'refactor' });
}

async function executeObservation({
  check,
  project,
  qualify,
  skipActual,
}: ExecuteInput): Promise<Observation> {
  if (qualify) {
    await verifyControls(project);
  }

  if (skipActual) {
    return { passed: true, value: check.expected };
  }

  return await readObservation({ project, variant: 'actual' });
}

/** Qualify a checker and compare its observation on the exact submitted code. */
export async function observe({
  check,
  files,
  projectRoot,
  qualify,
  skipActual = false,
}: ObserveInput): Promise<Observation> {
  const temporary = tmpdir();
  const workPrefix = join(temporary, 'tenet-observe-');
  const tmpPrefix = join(temporary, 'tenet-probe-tmp-');
  const work = await mkdtemp(workPrefix);
  const probeTmp = await mkdtemp(tmpPrefix);

  try {
    const project = await prepareProject({ check, files, probeTmp, projectRoot, work });

    return await executeObservation({ check, project, qualify, skipActual });
  } finally {
    await rm(work, { force: true, recursive: true });
    await rm(probeTmp, { force: true, recursive: true });
  }
}
