import { CheckerError } from '../checker-error.mts';
import { buildEditError } from './control-diagnostics.mts';

const MAX_CONTROL_EDITS = 8;
const MAX_EDIT_BYTES = 65_536;

type ControlEdit = Readonly<{ find: string; replace: string }>;

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

type ApplySingleInput = Readonly<{
  current: string;
  edit: ControlEdit;
  label: string;
}>;

type ApplyEditsInput = Readonly<{
  edits: readonly unknown[];
  label: string;
  limit: number;
  source: string;
}>;

type NormalizeInput = Readonly<{
  current: string;
  find: string;
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

function normalizeEditTarget({ current, find }: NormalizeInput): string {
  if (current.includes(find)) {
    return find;
  }

  if (find.includes('\r\n') && !current.includes('\r\n')) {
    return find.replaceAll('\r\n', '\n');
  }

  return find;
}

function applySingleEdit({ current, edit, label }: ApplySingleInput): string {
  const target = normalizeEditTarget({ current, find: edit.find });
  const first = current.indexOf(target);

  if (first === -1) {
    throw buildEditError({ current, find: edit.find, kind: 'missing', label });
  }

  const second = current.indexOf(target, first + 1);

  if (second !== -1) {
    throw buildEditError({ current, find: edit.find, kind: 'ambiguous', label });
  }

  const prefix = current.slice(0, first);
  const suffix = current.slice(first + target.length);

  return `${prefix}${edit.replace}${suffix}`;
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

type OversizedControlInput = Readonly<{
  bytes: number;
  controlName: string;
  limit: number;
}>;

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
