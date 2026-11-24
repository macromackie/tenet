import { Option } from 'effect';
import type { Check } from '../types.d.mts';
import { hashSource } from './hash-source.mts';

type BuildCheckEntryInput = Readonly<{
  check: Check;
  includeControls?: boolean;
  scriptFile?: string;
  scriptHash?: string;
  test?: string;
}>;

type ReadControlsInput = Readonly<{
  check: Check;
  includeControls: boolean;
}>;

type ControlHashInput = Readonly<{
  controlHash?: string;
  defect?: string;
  good?: string;
  refactor?: string;
}>;

type OptionalTextInput = Readonly<{
  fallback?: string;
  preferred?: string;
}>;

type OptionalControls = Pick<Check, 'brownfield' | 'controlHash' | 'defect' | 'good' | 'refactor'>;
type OptionalScripts = Pick<Check, 'scriptFile' | 'scriptHash' | 'test' | 'testFile'>;

function computeControlHash({
  controlHash,
  defect = '',
  good = '',
  refactor = '',
}: ControlHashInput): Option.Option<string> {
  if (controlHash !== undefined && controlHash.length > 0) {
    return Option.some(controlHash);
  }

  if (good.length === 0 && defect.length === 0 && refactor.length === 0) {
    return Option.none();
  }

  const encoded = JSON.stringify([good, defect, refactor]);
  const digest = hashSource(encoded);

  return Option.some(digest);
}

function readCheckTarget(
  check: Check,
): Pick<Check, 'expected' | 'id' | 'promise' | 'runtime' | 'subject'> {
  return {
    expected: check.expected,
    id: check.id,
    promise: check.promise,
    runtime: check.runtime,
    subject: check.subject,
  };
}

function pickBrownfield(brownfield?: boolean): Pick<Check, 'brownfield'> {
  if (brownfield !== undefined) {
    return { brownfield };
  }

  return {};
}

function pickControlHash(controlHash: Option.Option<string>): Pick<Check, 'controlHash'> {
  if (Option.isSome(controlHash)) {
    return { controlHash: controlHash.value };
  }

  return {};
}

function pickDefect(defect?: string): Pick<Check, 'defect'> {
  if (defect !== undefined) {
    return { defect };
  }

  return {};
}

function pickGood(good?: string): Pick<Check, 'good'> {
  if (good !== undefined) {
    return { good };
  }

  return {};
}

function pickRefactor(refactor?: string): Pick<Check, 'refactor'> {
  if (refactor !== undefined) {
    return { refactor };
  }

  return {};
}

function readCheckControls({ check, includeControls }: ReadControlsInput): OptionalControls {
  const controlHash = computeControlHash(check);

  const meta = {
    ...pickBrownfield(check.brownfield),
    ...pickControlHash(controlHash),
  };

  if (!includeControls) {
    return meta;
  }

  return {
    ...meta,
    ...pickDefect(check.defect),
    ...pickGood(check.good),
    ...pickRefactor(check.refactor),
  };
}

function resolveOptionalText({ fallback, preferred }: OptionalTextInput): Option.Option<string> {
  if (preferred !== undefined) {
    return Option.some(preferred);
  }

  return Option.fromUndefinedOr(fallback);
}

function pickScriptFile(scriptFile: Option.Option<string>): Pick<Check, 'scriptFile'> {
  if (Option.isSome(scriptFile)) {
    return { scriptFile: scriptFile.value };
  }

  return {};
}

function pickScriptHash(scriptHash: Option.Option<string>): Pick<Check, 'scriptHash'> {
  if (Option.isSome(scriptHash)) {
    return { scriptHash: scriptHash.value };
  }

  return {};
}

function pickTest(test?: string): Pick<Check, 'test'> {
  if (test !== undefined) {
    return { test };
  }

  return {};
}

function pickTestFile(testFile?: string): Pick<Check, 'testFile'> {
  if (testFile !== undefined) {
    return { testFile };
  }

  return {};
}

function readScriptFields({
  check,
  scriptFile,
  scriptHash,
  test,
}: BuildCheckEntryInput): OptionalScripts {
  const resolvedFile = resolveOptionalText({ fallback: check.scriptFile, preferred: scriptFile });
  const resolvedHash = resolveOptionalText({ fallback: check.scriptHash, preferred: scriptHash });

  return {
    ...pickScriptFile(resolvedFile),
    ...pickScriptHash(resolvedHash),
    ...pickTest(test),
    ...pickTestFile(check.testFile),
  };
}

/** Construct a slim on-disk or hydrated in-memory Check entry with canonical key ordering. */
export function buildCheckEntry(input: BuildCheckEntryInput): Check {
  const { check, includeControls = true } = input;
  const target = readCheckTarget(check);
  const controls = readCheckControls({ check, includeControls });
  const scripts = readScriptFields(input);

  return { ...target, ...controls, ...scripts };
}
