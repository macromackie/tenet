import { dirname, join } from 'node:path';
import { Option, Result } from 'effect';
import { classifyResult } from '../classify-result.mts';
import type { Check, Pin } from '../types.d.mts';
import { allocateScriptFile } from './allocate-script-file.mts';
import { buildCheckEntry } from './build-check-entry.mts';
import { hashSource } from './hash-source.mts';
import { readRegular } from './read-regular.mts';
import { writeTextAtomic } from './write-text-atomic.mts';

const MAX_SCRIPT_BYTES = 262_144;

type WritePinScriptsInput = Readonly<{
  lockPath: string;
  pin: Pin;
  priorPin?: Pin;
  root: string;
}>;

type WritePinScriptsResult = Readonly<{
  diskPin: Pin;
  handEditedIds: ReadonlySet<string>;
  hydratedPin: Pin;
}>;

type ReadOptionalScriptInput = Readonly<{ path: string; root: string }>;

type PriorIndex = Readonly<{
  priorFileChecks: ReadonlyMap<string, Check>;
  priorIdChecks: ReadonlyMap<string, Check>;
}>;

type WriteOneCheckInput = PriorIndex &
  Readonly<{ check: Check; lockDir: string; root: string; usedNames: ReadonlySet<string> }>;

type WriteOneCheckResult = Readonly<{
  diskCheck: Check;
  handEdited: boolean;
  hydratedCheck: Check;
  scriptFile?: string;
}>;

type PreserveEditInput = Readonly<{
  check: Check;
  priorHash?: string;
  root: string;
  scriptFile: string;
  scriptPath: string;
}>;

type WriteFreshScriptInput = Readonly<{
  check: Check;
  root: string;
  scriptFile: string;
  scriptPath: string;
  text: string;
}>;

type WriteState = Readonly<{
  diskChecks: readonly Check[];
  handEditedIds: ReadonlySet<string>;
  hydratedChecks: readonly Check[];
  usedNames: ReadonlySet<string>;
}>;

type RecordWrittenInput = Readonly<{
  checkId: string;
  result: WriteOneCheckResult;
  state: WriteState;
}>;

type AddIfPresentInput = Readonly<{ item?: string; items: ReadonlySet<string> }>;

async function readOptionalScript({
  path,
  root,
}: ReadOptionalScriptInput): Promise<Option.Option<string>> {
  async function runRead(): Promise<string> {
    return await readRegular({ limit: MAX_SCRIPT_BYTES, path, root });
  }

  const outcome = await classifyResult({ message: 'could not read existing script', run: runRead });

  if (Result.isSuccess(outcome)) {
    return Option.some(outcome.success);
  }

  const { cause } = outcome.failure;

  if (cause instanceof Error && 'code' in cause && cause.code === 'ENOENT') {
    return Option.none();
  }

  throw outcome.failure;
}

function indexPriorChecks(priorPin?: Pin): PriorIndex {
  const priorIdChecks = new Map<string, Check>();
  const priorFileChecks = new Map<string, Check>();

  if (priorPin === undefined) {
    return { priorFileChecks, priorIdChecks };
  }

  for (const item of priorPin.bundle.checks) {
    priorIdChecks.set(item.id, item);

    if (item.scriptFile !== undefined) {
      priorFileChecks.set(item.scriptFile, item);
    }
  }

  return { priorFileChecks, priorIdChecks };
}

async function preserveIfHandEdited({
  check,
  priorHash,
  root,
  scriptFile,
  scriptPath,
}: PreserveEditInput): Promise<Option.Option<WriteOneCheckResult>> {
  if (priorHash === undefined) {
    return Option.none();
  }

  const existing = await readOptionalScript({ path: scriptPath, root });

  if (Option.isNone(existing) || hashSource(existing.value) === priorHash) {
    return Option.none();
  }

  const diskCheck = buildCheckEntry({
    check,
    includeControls: false,
    scriptFile,
    scriptHash: priorHash,
  });

  const hydratedCheck = buildCheckEntry({
    check,
    includeControls: true,
    scriptFile,
    scriptHash: priorHash,
    test: existing.value,
  });

  return Option.some({ diskCheck, handEdited: true, hydratedCheck, scriptFile });
}

async function writeFreshScript({
  check,
  root,
  scriptFile,
  scriptPath,
  text,
}: WriteFreshScriptInput): Promise<WriteOneCheckResult> {
  await writeTextAtomic({ path: scriptPath, root, text });

  const scriptHash = hashSource(text);
  const diskCheck = buildCheckEntry({ check, includeControls: false, scriptFile, scriptHash });

  const hydratedCheck = buildCheckEntry({
    check,
    includeControls: true,
    scriptFile,
    scriptHash,
    test: text,
  });

  return { diskCheck, handEdited: false, hydratedCheck, scriptFile };
}

function buildNonScriptResult(check: Check): WriteOneCheckResult {
  const diskCheck = buildCheckEntry({ check, includeControls: false });
  const hydratedCheck = buildCheckEntry({ check, includeControls: true });

  return { diskCheck, handEdited: false, hydratedCheck };
}

async function writeOneCheck({
  check,
  lockDir,
  priorFileChecks,
  priorIdChecks,
  root,
  usedNames,
}: WriteOneCheckInput): Promise<WriteOneCheckResult> {
  if (check.test === undefined || check.test.length === 0) {
    return buildNonScriptResult(check);
  }

  const priorByCheckId = priorIdChecks.get(check.id);
  const scriptFile = allocateScriptFile({ check, priorCheck: priorByCheckId, usedNames });
  const scriptPath = join(lockDir, scriptFile);
  const priorHash = (priorByCheckId ?? priorFileChecks.get(scriptFile))?.scriptHash ?? check.scriptHash;
  const preserved = await preserveIfHandEdited({ check, priorHash, root, scriptFile, scriptPath });

  if (Option.isSome(preserved)) {
    return preserved.value;
  }

  return await writeFreshScript({ check, root, scriptFile, scriptPath, text: check.test });
}

function addIfPresent({ item, items }: AddIfPresentInput): ReadonlySet<string> {
  if (item === undefined) {
    return items;
  }

  return new Set([...items, item]);
}

function recordHandEdit({ checkId, result, state }: RecordWrittenInput): ReadonlySet<string> {
  if (!result.handEdited) {
    return state.handEditedIds;
  }

  return new Set([...state.handEditedIds, checkId]);
}

function recordWrittenCheck(input: RecordWrittenInput): WriteState {
  const { result, state } = input;
  const usedNames = addIfPresent({ item: result.scriptFile, items: state.usedNames });
  const handEditedIds = recordHandEdit(input);

  return {
    diskChecks: [...state.diskChecks, result.diskCheck],
    handEditedIds,
    hydratedChecks: [...state.hydratedChecks, result.hydratedCheck],
    usedNames,
  };
}

/** Write runnable check scripts alongside `.lock.json`, preserving any hand-edited files on disk. */
export async function writePinScripts({
  lockPath,
  pin,
  priorPin,
  root,
}: WritePinScriptsInput): Promise<WritePinScriptsResult> {
  const lockDir = dirname(lockPath);
  const priorIndex = indexPriorChecks(priorPin);

  let state: WriteState = {
    diskChecks: [],
    handEditedIds: new Set<string>(),
    hydratedChecks: [],
    usedNames: new Set<string>(),
  };

  for (const check of pin.bundle.checks) {
    // Materialize scripts sequentially to guarantee deterministic filename allocation.
    // oxlint-disable-next-line no-await-in-loop
    const result = await writeOneCheck({
      ...priorIndex,
      check,
      lockDir,
      root,
      usedNames: state.usedNames,
    });

    state = recordWrittenCheck({ checkId: check.id, result, state });
  }

  return {
    diskPin: { ...pin, bundle: { checks: state.diskChecks, open: pin.bundle.open } },
    handEditedIds: state.handEditedIds,
    hydratedPin: { ...pin, bundle: { checks: state.hydratedChecks, open: pin.bundle.open } },
  };
}
