import { Option, Result } from 'effect';
import { classifyResult } from '../../classify-result.mts';
import { isMissingExternalDepMessage, observe } from '../../sandbox/index.mts';
import type { Check, Finding, Inventory, WitnessRepro } from '../../types.d.mts';
import { buildFinding } from '../build-finding.mts';

const MAX_FIELD_BYTES = 131_072;
const MAX_REPRO_CODE_CHARS = 4_000;
const MAX_REPRO_VALUE_CHARS = 512;
const ID_PATTERN = /^[a-z][a-z0-9\-]{0,63}$/v;

const CONTROL_PARSE_MARKERS = [
  '"good"',
  '"refactor"',
  'refactor matched good',
  'three distinct alternatives',
] as const;

type ParseWitnessInput = Readonly<{
  allowBrownfieldWitness: boolean;
  contractName: string;
  current: Inventory;
  detail: string;
  kind: 'parse';
  projectRoot?: string;
  raw: unknown;
}>;

type QualifyWitnessInput = Readonly<{
  allowBrownfieldWitness: boolean;
  candidate: Check;
  contractName: string;
  detail: string;
  files: Inventory['runtimeFiles'];
  kind: 'qualify';
  projectRoot?: string;
}>;

type BrownfieldWitnessInput = ParseWitnessInput | QualifyWitnessInput;

type ProbeWitnessInput = Readonly<{
  check: Check;
  contractName: string;
  files: Inventory['runtimeFiles'];
  goodPassed: boolean;
  projectRoot?: string;
}>;

type BuildWitnessInput = Readonly<{
  check: Check;
  contractName: string;
  goodPassed: boolean;
  observedValue: unknown;
}>;

type AcceptableWitnessInput = Readonly<{ goodPassed: boolean; observedValue: unknown }>;
type ExtractCheckInput = Readonly<{ files: Inventory['files']; raw: unknown }>;
type ReadStringFieldInput = Readonly<{ fields: ReadonlyMap<string, unknown>; key: string }>;
type BuildReproInput = Readonly<{ check: Check; observedValue: unknown }>;

type CheckParts = Readonly<{
  expected: unknown; id: string; promise: string; runtime: Check['runtime']; subject: string; test: string;
}>;

function isControlOnlyParseError(detail: string): boolean {
  if (detail.includes('"defect"')) {
    return false;
  }

  return CONTROL_PARSE_MARKERS.some((marker) => detail.includes(marker));
}

function isControlOnlyQualifyError(detail: string): boolean {
  if (isMissingExternalDepMessage(detail) || detail.includes('"defect"')) {
    return false;
  }

  return detail.includes('"good"') || detail.includes('"refactor"');
}

function readBoundedField({ fields, key }: ReadStringFieldInput): Option.Option<string> {
  const value = fields.get(key);

  if (
    typeof value !== 'string' ||
    value.trim().length === 0 ||
    Buffer.byteLength(value) > MAX_FIELD_BYTES
  ) {
    return Option.none();
  }

  return Option.some(value);
}

function readRuntimeOption(value: unknown): Option.Option<Check['runtime']> {
  switch (value) {
    case 'go':
    case 'node':
    case 'python':
    case 'rust': {
      return Option.some(value);
    }
    default: {
      return Option.none();
    }
  }
}

function readObjectFields(raw: unknown): Option.Option<ReadonlyMap<string, unknown>> {
  if (raw === null || typeof raw !== 'object' || Array.isArray(raw)) {
    return Option.none();
  }

  const entries = Object.entries(raw);
  const fields = new Map<string, unknown>(entries);

  return Option.some(fields);
}

function readTextParts(
  fields: ReadonlyMap<string, unknown>,
): Option.Option<Readonly<{ id: string; promise: string; subject: string; test: string }>> {
  const id = readBoundedField({ fields, key: 'id' });
  const subject = readBoundedField({ fields, key: 'subject' });
  const promise = readBoundedField({ fields, key: 'promise' });
  const test = readBoundedField({ fields, key: 'test' });

  return Option.all({ id, promise, subject, test });
}

function readCheckParts(fields: ReadonlyMap<string, unknown>): Option.Option<CheckParts> {
  const texts = readTextParts(fields);
  const rawRuntime = fields.get('runtime');
  const runtime = readRuntimeOption(rawRuntime);

  if (Option.isNone(texts) || Option.isNone(runtime)) {
    return Option.none();
  }

  if (!ID_PATTERN.test(texts.value.id) || !fields.has('expected')) {
    return Option.none();
  }

  const expected = fields.get('expected');

  return Option.some({ ...texts.value, expected, runtime: runtime.value });
}

function extractProbeCheck({ files, raw }: ExtractCheckInput): Option.Option<Check> {
  const fieldsOption = readObjectFields(raw);
  const partsOption = Option.flatMap(fieldsOption, (fields) => readCheckParts(fields));

  if (Option.isNone(partsOption)) {
    return Option.none();
  }

  const parts = partsOption.value;
  const source = files.get(parts.subject);

  if (source === undefined) {
    return Option.none();
  }

  return Option.some({ ...parts, defect: source, good: source, refactor: source });
}

function isPropertyCounterexample(value: unknown): boolean {
  if (value === null || typeof value !== 'object' || Array.isArray(value)) {
    return false;
  }

  const entries = Object.entries(value);
  const fields = new Map<string, unknown>(entries);
  const ok = fields.get('ok');
  const passed = fields.get('passed');

  return ok === false && typeof passed === 'number' && passed >= 1;
}

function buildRepro({ check, observedValue }: BuildReproInput): Option.Option<WitnessRepro> {
  const code = check.test?.trim() ?? '';

  if (code.length === 0) {
    return Option.none();
  }

  const expected = JSON.stringify(check.expected).slice(0, MAX_REPRO_VALUE_CHARS);
  const observed = JSON.stringify(observedValue).slice(0, MAX_REPRO_VALUE_CHARS);
  const boundedCode = code.slice(0, MAX_REPRO_CODE_CHARS);

  return Option.some({ code: boundedCode, expected, observed, runtime: check.runtime });
}

function hasAcceptableWitness({ goodPassed, observedValue }: AcceptableWitnessInput): boolean {
  return goodPassed || isPropertyCounterexample(observedValue) || observedValue !== undefined;
}

function buildWitnessFinding({
  check,
  contractName,
  goodPassed,
  observedValue,
}: BuildWitnessInput): Option.Option<Finding> {
  const reproOption = buildRepro({ check, observedValue });

  if (Option.isNone(reproOption) || !hasAcceptableWitness({ goodPassed, observedValue })) {
    return Option.none();
  }

  const expectedText = JSON.stringify(check.expected);
  const observedText = JSON.stringify(observedValue);
  const message = `[unverified brownfield witness] ${check.promise} (expected ${expectedText}, got ${observedText})`;

  const finding = buildFinding({
    brownfield_unverified: true,
    check: check.id,
    contract: contractName,
    message,
    outcome: 'failed',
    repro: reproOption.value,
    subject: check.subject,
  });

  return Option.some(finding);
}

async function observeUnmodifiedWitness({
  check,
  contractName,
  files,
  goodPassed,
  projectRoot,
}: ProbeWitnessInput): Promise<Option.Option<Finding>> {
  async function runActual() {
    return await observe({ check, files, projectRoot, qualify: false });
  }

  const attempt = await classifyResult({
    message: 'brownfield witness probe failed',
    run: runActual,
  });

  if (Result.isFailure(attempt) || attempt.success.passed) {
    return Option.none();
  }

  return buildWitnessFinding({
    check,
    contractName,
    goodPassed,
    observedValue: attempt.success.value,
  });
}

async function tryParseWitness(input: ParseWitnessInput): Promise<Option.Option<Finding>> {
  if (!isControlOnlyParseError(input.detail)) {
    return Option.none();
  }

  const probeCheck = extractProbeCheck({ files: input.current.files, raw: input.raw });

  if (Option.isNone(probeCheck)) {
    return Option.none();
  }

  return await observeUnmodifiedWitness({
    check: probeCheck.value,
    contractName: input.contractName,
    files: input.current.runtimeFiles,
    goodPassed: false,
    projectRoot: input.projectRoot,
  });
}

/** In audit mode, salvage a live failing witness when only good/refactor control patching or qualification failed. */
export async function tryBrownfieldWitness(
  input: BrownfieldWitnessInput,
): Promise<Option.Option<Finding>> {
  if (!input.allowBrownfieldWitness) {
    return Option.none();
  }

  if (input.kind === 'parse') {
    return await tryParseWitness(input);
  }

  if (!isControlOnlyQualifyError(input.detail)) {
    return Option.none();
  }

  const goodPassed = input.detail.includes('"refactor"');

  return await observeUnmodifiedWitness({
    check: input.candidate,
    contractName: input.contractName,
    files: input.files,
    goodPassed,
    projectRoot: input.projectRoot,
  });
}
