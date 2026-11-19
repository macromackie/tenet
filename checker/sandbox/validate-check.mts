import { CheckerError } from '../checker-error.mts';
import type { Check } from '../types.d.mts';

type CandidateInput = Readonly<{ check: unknown; files: ReadonlyMap<string, string> }>;
type Input = Readonly<{ fields: ReadonlyMap<string, unknown>; files: ReadonlyMap<string, string> }>;
const MAX_FIELD_BYTES = 65_536;
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

function readTarget({ fields, files }: Input): Omit<Check, 'good' | 'defect' | 'refactor'> {
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

function readControls(fields: Input['fields']): Pick<Check, 'good' | 'defect' | 'refactor'> {
  const good = readField({ fields, key: 'good' });
  const defect = readField({ fields, key: 'defect' });
  const refactor = readField({ fields, key: 'refactor' });

  if (good === defect || refactor === defect || good === refactor) {
    throw new CheckerError({ message: 'qualification requires three distinct alternatives' });
  }

  return { defect, good, refactor };
}

/** Decode the observation, controls and exact project paths supplied by the planner. */
export function validate({ check, files }: CandidateInput): Check {
  const fields = readFields(check);
  const base = { ...readTarget({ fields, files }), ...readControls(fields) };

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
