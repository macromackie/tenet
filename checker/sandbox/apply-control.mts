import { CheckerError } from '../checker-error.mts';

const MAX_CONTROL_EDITS = 8;
const MAX_EDIT_BYTES = 65_536;

type ControlEdit = Readonly<{ find: string; replace: string }>;

type ControlInput = Readonly<{
  control: unknown;
  limit: number;
  source: string;
}>;

type ReadEditTextInput = Readonly<{
  allowEmpty: boolean;
  value: unknown;
}>;

type ApplySingleInput = Readonly<{
  current: string;
  edit: ControlEdit;
}>;

type ApplyEditsInput = Readonly<{
  edits: readonly unknown[];
  limit: number;
  source: string;
}>;

function readEditText({ allowEmpty, value }: ReadEditTextInput): string {
  if (typeof value !== 'string' || Buffer.byteLength(value) > MAX_EDIT_BYTES) {
    throw new CheckerError({
      message: 'a control edit has an invalid or oversized text field',
    });
  }

  if (!allowEmpty && value.length === 0) {
    throw new CheckerError({ message: 'a control edit find target must be non-empty' });
  }

  return value;
}

function readEditMap(item: unknown): ReadonlyMap<string, unknown> {
  if (item === null || typeof item !== 'object' || Array.isArray(item)) {
    throw new CheckerError({ message: 'each control edit must be a find/replace object' });
  }

  const entries = Object.entries(item);

  return new Map<string, unknown>(entries);
}

function readControlEdit(item: unknown): ControlEdit {
  const fields = readEditMap(item);
  const rawFind = fields.get('find');
  const rawReplace = fields.get('replace');
  const find = readEditText({ allowEmpty: false, value: rawFind });
  const replace = readEditText({ allowEmpty: true, value: rawReplace });

  if (find === replace) {
    throw new CheckerError({ message: 'a control edit must change the matched source' });
  }

  return { find, replace };
}

function applySingleEdit({ current, edit }: ApplySingleInput): string {
  const first = current.indexOf(edit.find);

  if (first === -1) {
    throw new CheckerError({
      message: 'a control edit target was not found in the subject file',
    });
  }

  const second = current.indexOf(edit.find, first + 1);

  if (second !== -1) {
    throw new CheckerError({
      message: 'a control edit target matched multiple locations in the subject file',
    });
  }

  const prefix = current.slice(0, first);
  const suffix = current.slice(first + edit.find.length);

  return `${prefix}${edit.replace}${suffix}`;
}

function applyEdits({ edits, limit, source }: ApplyEditsInput): string {
  if (edits.length > MAX_CONTROL_EDITS) {
    throw new CheckerError({ message: 'a control exceeds the maximum number of edits' });
  }

  let current = source;

  for (const item of edits) {
    const edit = readControlEdit(item);

    current = applySingleEdit({ current, edit });

    if (Buffer.byteLength(current) > limit) {
      throw new CheckerError({ message: 'materialized control exceeds the bounded source limit' });
    }
  }

  return current;
}

/** Materialize either a full-file control string or targeted find/replace edits against the subject. */
export function applyControl({ control, limit, source }: ControlInput): string {
  if (typeof control === 'string') {
    if (control.trim().length === 0 || Buffer.byteLength(control) > limit) {
      throw new CheckerError({
        message: 'the generated checker is missing a bounded test, promise or control',
      });
    }

    return control;
  }

  if (Array.isArray(control)) {
    return applyEdits({ edits: control, limit, source });
  }

  throw new CheckerError({
    message: 'a control must be a full source string or an array of find/replace edits',
  });
}
