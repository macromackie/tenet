import { CheckerError } from '../checker-error.mts';
import type { Check } from '../types.d.mts';
import { applyControl } from './apply-control.mts';

type CandidateInput = Readonly<{ check: unknown; files: ReadonlyMap<string, string> }>;

type Input = Readonly<{
  fields: ReadonlyMap<string, unknown>;
  files: ReadonlyMap<string, string>;
}>;

type ControlInput = Readonly<{ fields: Input['fields']; source: string }>;

type NativeInput = Readonly<{
  base: Omit<Check, 'test' | 'testFile'>;
  fields: Input['fields'];
  files: Input['files'];
}>;

const MAX_FIELD_BYTES = 131_072;
const ID_PATTERN = /^[a-z][a-z0-9\-]{0,63}$/v;

function readFields(candidate: unknown): ReadonlyMap<string, unknown> {
  if (candidate === null || typeof candidate !== 'object' || Array.isArray(candidate)) {
    throw new CheckerError({ message: 'the generated checker must be an object' });
  }

  const entries = Object.entries(candidate);
  const fields: ReadonlyMap<string, unknown> = new Map(entries);

  return fields;
}

function readText(value: unknown): string {
  if (
    typeof value !== 'string' ||
    value.trim().length === 0 ||
    Buffer.byteLength(value) > MAX_FIELD_BYTES
  ) {
    throw new CheckerError({
      message: 'the generated checker is missing a bounded test, promise or control',
    });
  }

  return value;
}

function readRuntime(value: unknown): Check['runtime'] {
  switch (value) {
    case 'node':
    case 'python':
    case 'rust': {
      return value;
    }
    default: {
      throw new CheckerError({ message: 'generated checker has an unsupported runtime' });
    }
  }
}

type FieldInput = Readonly<{ fields: Input['fields']; key: string }>;

function readField({ fields, key }: FieldInput): string {
  const raw = fields.get(key);

  return readText(raw);
}

function readTarget({ fields, files }: Input): Omit<Check, 'defect' | 'good' | 'refactor'> {
  const id = readField({ fields, key: 'id' });
  const subject = readField({ fields, key: 'subject' });

  if (!ID_PATTERN.test(id) || !files.has(subject) || !fields.has('expected')) {
    throw new CheckerError({
      message: 'the generated checker has an invalid target or expected result',
    });
  }

  const selectedRuntime = fields.get('runtime');

  return {
    expected: fields.get('expected'),
    id,
    promise: readField({ fields, key: 'promise' }),
    runtime: readRuntime(selectedRuntime),
    subject,
  };
}

type DistinctControlsInput = Pick<Check, 'defect' | 'good' | 'refactor'>;

function assertDistinctControls({ defect, good, refactor }: DistinctControlsInput): void {
  if (good === refactor && good !== defect) {
    throw new CheckerError({
      message:
        'refactor matched good; rewrite refactor as a distinct behavior-preserving edit (for example, extract a helper or invert the condition)',
    });
  }

  if (good === defect || refactor === defect) {
    throw new CheckerError({ message: 'qualification requires three distinct alternatives' });
  }
}

function readControls({ fields, source }: ControlInput): Pick<Check, 'defect' | 'good' | 'refactor'> {
  const rawGood = fields.get('good');
  const rawDefect = fields.get('defect');
  const rawRefactor = fields.get('refactor');
  const good = applyControl({ control: rawGood, label: 'good', limit: MAX_FIELD_BYTES, source });

  const defect = applyControl({
    control: rawDefect,
    label: 'defect',
    limit: MAX_FIELD_BYTES,
    source,
  });

  const refactor = applyControl({
    control: rawRefactor,
    label: 'refactor',
    limit: MAX_FIELD_BYTES,
    source,
  });

  assertDistinctControls({ defect, good, refactor });

  return { defect, good, refactor };
}

function attachEntry({ base, fields, files }: NativeInput): Check {
  if (!fields.has('testFile')) {
    return { ...base, test: readField({ fields, key: 'test' }) };
  }

  const testFile = readField({ fields, key: 'testFile' });
  const isNative = base.runtime !== 'rust' && base.expected === true;
  const hasTest = files.has(testFile) && testFile !== base.subject;

  if (!hasTest || !isNative) {
    throw new CheckerError({
      message: 'select a separate existing native test file that reports assertion outcomes',
    });
  }

  return { ...base, testFile };
}

/** Decode the observation, materialize controls and verify project paths supplied by the planner. */
export function validate({ check, files }: CandidateInput): Check {
  const fields = readFields(check);
  const target = readTarget({ fields, files });
  const source = files.get(target.subject);

  if (source === undefined) {
    throw new CheckerError({ message: 'the generated checker target is missing from source' });
  }

  const base = { ...target, ...readControls({ fields, source }) };

  return attachEntry({ base, fields, files });
}
