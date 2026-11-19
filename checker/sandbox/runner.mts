import { mkdir, mkdtemp, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { dirname, join, resolve, sep } from 'node:path';
import { isDeepStrictEqual } from 'node:util';
import { Effect } from 'effect';
import { CheckerError } from '../checker-error.mts';
import type { Check } from '../types.d.mts';
import { createSandbox } from './runner-sandbox.mts';

type ObserveInput = Readonly<{
  files: ReadonlyMap<string, string>;
  check: Check;
  qualify: boolean;
}>;

type Project = Omit<ObserveInput, 'qualify'> & Readonly<{ work: string }>;
type Staged = Project & Readonly<{ run: () => Promise<unknown> }>;
type Case = 'good' | 'defect' | 'refactor' | 'actual';
type SampleInput = Readonly<{ project: Staged; variant: Case }>;
type Observation = Readonly<{ passed: boolean; value: unknown }>;
type SourceInput = Readonly<{ work: string; path: string; source: string }>;

async function copySource({ work, path, source }: SourceInput): Promise<void> {
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

  return { ...project, run: await createSandbox({ check: project.check, work: project.work }) };
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

async function readObservation({ project, variant }: SampleInput): Promise<Observation> {
  await restoreSubject({ project, variant });

  const { check } = project;

  function createObservationError(cause: unknown): CheckerError {
    const name = JSON.stringify(variant);

    if (cause instanceof Error) {
      const detail = `could not observe ${name} variant: ${cause.message}`;

      return new CheckerError({ cause, message: detail });
    }

    const message = `could not observe ${name} variant`;

    return new CheckerError({ cause, message });
  }

  const attempt = Effect.tryPromise({ catch: createObservationError, try: project.run });
  const value = await Effect.runPromise(attempt);

  return { passed: isDeepStrictEqual(value, check.expected), value };
}

async function qualifyControl({ project, variant }: SampleInput): Promise<void> {
  const result = await readObservation({ project, variant });

  if (result.passed !== (variant !== 'defect')) {
    const name = JSON.stringify(variant);
    const message = `test did not distinguish its ${name} control`;

    throw new CheckerError({ message });
  }
}

async function verifyControls(project: Staged): Promise<void> {
  await qualifyControl({ project, variant: 'good' });
  await qualifyControl({ project, variant: 'defect' });
  await qualifyControl({ project, variant: 'refactor' });
}

/** Qualify a checker and compare its observation on the exact submitted code. */
export async function observe({ files, check, qualify }: ObserveInput): Promise<Observation> {
  const temporary = tmpdir();
  const prefix = join(temporary, 'tenet-observe-');
  const work = await mkdtemp(prefix);

  try {
    const project = await prepareProject({ check, files, work });

    if (qualify) {
      await verifyControls(project);
    }

    return await readObservation({ project, variant: 'actual' });
  } finally {
    await rm(work, { force: true, recursive: true });
  }
}
