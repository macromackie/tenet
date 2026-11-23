import { Option } from 'effect';
import { CheckerError } from '../../checker-error.mts';
import { applySingleEdit, type ControlEdit } from './match-control-edit.mts';

const MAX_CONTROL_EDITS = 8;
const MAX_EDIT_BYTES = 65_536;

type ControlInput = Readonly<{
  control: unknown;
  label?: string;
  limit: number;
  source: string;
}>;

type ReadEditTextInput = Readonly<{
  allowEmpty: boolean;
  value: unknown;
}>;

type ApplyEditsInput = Readonly<{
  edits: readonly unknown[];
  label: string;
  limit: number;
  source: string;
}>;

type LimitCheckInput = Readonly<{
  current: string;
  label: string;
  limit: number;
}>;

type StringControlInput = Readonly<{
  control: string;
  label: string;
  limit: number;
}>;

type OversizedControlInput = Readonly<{
  bytes: number;
  controlName: string;
  limit: number;
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

function readOptionalHint(value: unknown): Option.Option<number> {
  if (value === undefined) {
    return Option.none();
  }

  if (typeof value === 'number' && Number.isInteger(value) && value >= 1) {
    return Option.some(value);
  }

  throw new CheckerError({
    message: 'a control edit line or occurrence hint must be a positive integer',
  });
}

function readEditMap(item: unknown): ReadonlyMap<string, unknown> {
  if (item === null || typeof item !== 'object' || Array.isArray(item)) {
    throw new CheckerError({ message: 'each control edit must be a find/replace object' });
  }

  const entries = Object.entries(item);

  return new Map<string, unknown>(entries);
}

type HintPair = Readonly<{ line?: number; occurrence?: number }>;

function readEditHints(fields: ReadonlyMap<string, unknown>): HintPair {
  const rawLine = fields.get('line');
  const rawOccurrence = fields.get('occurrence');
  const lineOption = readOptionalHint(rawLine);
  const occurrenceOption = readOptionalHint(rawOccurrence);

  return {
    line: Option.getOrUndefined(lineOption),
    occurrence: Option.getOrUndefined(occurrenceOption),
  };
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

  const hints = readEditHints(fields);

  return { find, ...hints, replace };
}

function assertWithinLimit({ current, label, limit }: LimitCheckInput): void {
  if (Buffer.byteLength(current) <= limit) {
    return;
  }

  const controlName = JSON.stringify(label);
  const message = `materialized control ${controlName} exceeds the bounded source limit`;

  throw new CheckerError({ message });
}

function applyEdits({ edits, label, limit, source }: ApplyEditsInput): string {
  if (edits.length > MAX_CONTROL_EDITS) {
    const controlName = JSON.stringify(label);
    const message = `control ${controlName} exceeds the maximum number of edits`;

    throw new CheckerError({ message });
  }

  let current = source;

  for (const item of edits) {
    const edit = readControlEdit(item);

    current = applySingleEdit({ current, edit, label });

    assertWithinLimit({ current, label, limit });
  }

  return current;
}

function buildOversizedControlError({
  bytes,
  controlName,
  limit,
}: OversizedControlInput): CheckerError {
  const sizeText = JSON.stringify(bytes);
  const limitText = JSON.stringify(limit);
  const message = `control ${controlName} string is ${sizeText} bytes (limit ${limitText}); use a compact find/replace edit array instead of a full-file string`;

  return new CheckerError({ message });
}

function applyStringControl({ control, label, limit }: StringControlInput): string {
  const controlName = JSON.stringify(label);
  const bytes = Buffer.byteLength(control);

  if (control.trim().length === 0) {
    const message = `control ${controlName} must be a non-empty source string or find/replace edit array`;

    throw new CheckerError({ message });
  }

  if (bytes > limit) {
    throw buildOversizedControlError({ bytes, controlName, limit });
  }

  return control;
}

/** Materialize either a full-file control string or targeted find/replace edits against the subject. */
export function applyControl({ control, label = 'control', limit, source }: ControlInput): string {
  if (typeof control === 'string') {
    return applyStringControl({ control, label, limit });
  }

  if (Array.isArray(control)) {
    return applyEdits({ edits: control, label, limit, source });
  }

  const controlName = JSON.stringify(label);
  const message = `control ${controlName} must be a full source string or an array of find/replace edits`;

  throw new CheckerError({ message });
}
