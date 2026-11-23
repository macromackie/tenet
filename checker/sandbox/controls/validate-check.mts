import { CheckerError } from '../../checker-error.mts';
import type { Check } from '../../types.d.mts';
import { applyControl } from './apply-control.mts';

type CandidateInput = Readonly<{
  check: unknown;
  files: ReadonlyMap<string, string>;
  fromPin?: boolean;
}>;

type Input = Readonly<{
  fields: ReadonlyMap<string, unknown>;
  files: ReadonlyMap<string, string>;
}>;

type ControlInput = Readonly<{
  fields: Input['fields'];
  fromPin: boolean;
  source: string;
}>;

type ControlLimitInput = Readonly<{
  control: unknown;
  source: string;
}>;

type NativeInput = Readonly<{
  base: Omit<Check, 'test' | 'testFile'>;
  fields: Input['fields'];
  files: Input['files'];
}>;

type ControlsResult = Pick<Check, 'brownfield' | 'defect' | 'good' | 'refactor'>;

const MAX_FIELD_BYTES = 131_072;
const EDIT_LIMIT_PADDING = 32_768;
const ID_PATTERN = /^[a-z][a-z0-9\-]{0,63}$/v;
const NATIVE_TEST_RUNTIMES: ReadonlySet<string> = new Set(['node', 'python']);

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
    case 'go':
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

function readControlLimit({ control, source }: ControlLimitInput): number {
  if (Array.isArray(control)) {
    const padded = Buffer.byteLength(source) + EDIT_LIMIT_PADDING;

    return Math.max(MAX_FIELD_BYTES, padded);
  }

  return MAX_FIELD_BYTES;
}

type ResolveRefactorInput = Readonly<{
  allowBrownfield: boolean;
  defect: string;
  explicitBrownfield: boolean;
  good: string;
  rawRefactor: unknown;
  source: string;
}>;

function resolveRefactor({
  allowBrownfield,
  defect,
  explicitBrownfield,
  good,
  rawRefactor,
  source,
}: ResolveRefactorInput): ControlsResult {
  if (allowBrownfield && rawRefactor === undefined) {
    return { brownfield: true, defect, good, refactor: good };
  }

  const limit = readControlLimit({ control: rawRefactor, source });
  const refactor = applyControl({ control: rawRefactor, label: 'refactor', limit, source });

  if (allowBrownfield && refactor === good) {
    return { brownfield: true, defect, good, refactor };
  }

  assertDistinctControls({ defect, good, refactor });

  if (allowBrownfield && explicitBrownfield) {
    return { brownfield: true, defect, good, refactor };
  }

  return { defect, good, refactor };
}

type BuildControlInput = Readonly<{
  control: unknown;
  label: string;
  source: string;
}>;

function buildControl({ control, label, source }: BuildControlInput): string {
  const limit = readControlLimit({ control, source });

  return applyControl({ control, label, limit, source });
}

function readControls({ fields, fromPin, source }: ControlInput): ControlsResult {
  const rawGood = fields.get('good');
  const rawDefect = fields.get('defect');
  const rawRefactor = fields.get('refactor');
  const good = buildControl({ control: rawGood, label: 'good', source });
  const defect = buildControl({ control: rawDefect, label: 'defect', source });
  const explicitBrownfield = fields.get('brownfield') === true;
  const isBrownfieldOrigin = defect === source || (fromPin && explicitBrownfield);
  const allowBrownfield = good !== defect && isBrownfieldOrigin;

  return resolveRefactor({
    allowBrownfield,
    defect,
    explicitBrownfield,
    good,
    rawRefactor,
    source,
  });
}

function attachEntry({ base, fields, files }: NativeInput): Check {
  if (!fields.has('testFile')) {
    return { ...base, test: readField({ fields, key: 'test' }) };
  }

  const testFile = readField({ fields, key: 'testFile' });
  const isNative = NATIVE_TEST_RUNTIMES.has(base.runtime) && base.expected === true;
  const hasTest = files.has(testFile) && testFile !== base.subject;

  if (!hasTest || !isNative) {
    throw new CheckerError({
      message: 'select a separate existing native test file that reports assertion outcomes',
    });
  }

  return { ...base, testFile };
}

/** Decode the observation, materialize controls and verify project paths supplied by the planner. */
export function validate({ check, files, fromPin = false }: CandidateInput): Check {
  const fields = readFields(check);
  const target = readTarget({ fields, files });
  const source = files.get(target.subject);

  if (source === undefined) {
    throw new CheckerError({ message: 'the generated checker target is missing from source' });
  }

  const controls = readControls({ fields, fromPin, source });
  const base = { ...target, ...controls };

  return attachEntry({ base, fields, files });
}
