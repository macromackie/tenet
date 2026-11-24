import { Option } from 'effect';
import type { Check } from '../../types.d.mts';

const MAX_FIELD_BYTES = 131_072;

export type ControlsResult = Pick<
  Check,
  'brownfield' | 'controlHash' | 'defect' | 'good' | 'refactor'
>;

export type ScriptMeta = Readonly<{
  scriptFile?: string;
  scriptHash?: string;
}>;

export type PinMeta = Readonly<{
  controlHash: Option.Option<string>;
  scriptMeta: ScriptMeta;
  slimControls: Option.Option<ControlsResult>;
}>;

type PinMetaInput = Readonly<{
  fields: ReadonlyMap<string, unknown>;
  fromPin: boolean;
}>;

type PinTexts = Readonly<{
  controlHash: Option.Option<string>;
  scriptFile: Option.Option<string>;
  scriptHash: Option.Option<string>;
}>;

type BuildSlimInput = Readonly<{
  controlHash: Option.Option<string>;
  explicitBrownfield: boolean;
}>;

type SlimControlsInput = Readonly<{
  controlHash: Option.Option<string>;
  explicitBrownfield: boolean;
  fields: ReadonlyMap<string, unknown>;
  scriptFile: Option.Option<string>;
}>;

function readOptionalPinText(raw: unknown): Option.Option<string> {
  if (
    typeof raw !== 'string' ||
    raw.trim().length === 0 ||
    Buffer.byteLength(raw) > MAX_FIELD_BYTES
  ) {
    return Option.none();
  }

  return Option.some(raw);
}

function readPinTexts(fields: ReadonlyMap<string, unknown>): PinTexts {
  const rawControlHash = fields.get('controlHash');
  const rawScriptFile = fields.get('scriptFile');
  const rawScriptHash = fields.get('scriptHash');

  return {
    controlHash: readOptionalPinText(rawControlHash),
    scriptFile: readOptionalPinText(rawScriptFile),
    scriptHash: readOptionalPinText(rawScriptHash),
  };
}

function isOmittedControl(value: unknown): boolean {
  return value === undefined || value === '';
}

function hasOmittedControls(fields: ReadonlyMap<string, unknown>): boolean {
  const rawGood = fields.get('good');
  const rawDefect = fields.get('defect');
  const rawRefactor = fields.get('refactor');

  return isOmittedControl(rawGood) && isOmittedControl(rawDefect) && isOmittedControl(rawRefactor);
}

function buildSlimControls({ controlHash, explicitBrownfield }: BuildSlimInput): ControlsResult {
  if (explicitBrownfield && Option.isSome(controlHash)) {
    return { brownfield: true, controlHash: controlHash.value };
  }

  if (explicitBrownfield) {
    return { brownfield: true };
  }

  if (Option.isSome(controlHash)) {
    return { controlHash: controlHash.value };
  }

  return {};
}

function resolveSlimControls({
  controlHash,
  explicitBrownfield,
  fields,
  scriptFile,
}: SlimControlsInput): Option.Option<ControlsResult> {
  if (!hasOmittedControls(fields)) {
    return Option.none();
  }

  if (Option.isNone(scriptFile) && Option.isNone(controlHash)) {
    return Option.none();
  }

  const slim = buildSlimControls({ controlHash, explicitBrownfield });

  return Option.some(slim);
}

type BuildScriptMetaInput = Readonly<{
  scriptFile: Option.Option<string>;
  scriptHash: Option.Option<string>;
}>;

function buildScriptMeta({ scriptFile, scriptHash }: BuildScriptMetaInput): ScriptMeta {
  if (Option.isSome(scriptFile) && Option.isSome(scriptHash)) {
    return { scriptFile: scriptFile.value, scriptHash: scriptHash.value };
  }

  if (Option.isSome(scriptFile)) {
    return { scriptFile: scriptFile.value };
  }

  if (Option.isSome(scriptHash)) {
    return { scriptHash: scriptHash.value };
  }

  return {};
}

/** Read pin-only metadata (scriptFile, scriptHash, controlHash, and slim controls). */
export function readPinMeta({ fields, fromPin }: PinMetaInput): PinMeta {
  if (!fromPin) {
    return { controlHash: Option.none(), scriptMeta: {}, slimControls: Option.none() };
  }

  const { controlHash, scriptFile, scriptHash } = readPinTexts(fields);
  const explicitBrownfield = fields.get('brownfield') === true;
  const slimControls = resolveSlimControls({ controlHash, explicitBrownfield, fields, scriptFile });
  const scriptMeta = buildScriptMeta({ scriptFile, scriptHash });

  return { controlHash, scriptMeta, slimControls };
}
