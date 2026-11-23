import { Option, Result, Schema } from 'effect';
import { classifyResult } from '../classify-result.mts';
import { readPinScripts, readRecord } from '../storage/index.mts';
import type { Check } from '../types.d.mts';
import { Pin } from '../wire.mts';
import type { ReplayRepro, ReplayTarget } from './replay-wire.mts';

const decodePin = Schema.decodeUnknownOption(Pin, { onExcessProperty: 'error' });
const CheckRuntime = Schema.Literals(['node', 'python', 'rust', 'go']);
const decodeRuntime = Schema.decodeUnknownOption(CheckRuntime, { onExcessProperty: 'error' });
const ExpectedJson = Schema.fromJsonString(Schema.Unknown);
const decodeExpectedJson = Schema.decodeUnknownOption(ExpectedJson, { onExcessProperty: 'error' });

type ResolveReplayCheckInput = Readonly<{
  root: string;
  state: string;
  target: ReplayTarget;
}>;

type ReadPinnedCheckInput = Readonly<{
  checkId: string;
  lockPath?: string;
  pinRoot: string;
}>;

type ReadLockCheckInput = Readonly<{
  checkId: string;
  lockPath: string;
  pinRoot: string;
}>;

type BuildFallbackInput = Readonly<{
  repro: ReplayRepro;
  target: ReplayTarget;
}>;

function selectPinRoot({ root, state, target }: ResolveReplayCheckInput): string {
  if (target.pinRoot === 'root') {
    return root;
  }

  return state;
}

type ReadMatchedInput = Readonly<{
  checkId: string;
  lockPath: string;
  pin: Pin;
  pinRoot: string;
}>;

async function readMatchedCheck({
  checkId,
  lockPath,
  pin,
  pinRoot,
}: ReadMatchedInput): Promise<Option.Option<Check>> {
  const matched = pin.bundle.checks.find((item) => item.id === checkId);

  if (matched === undefined) {
    return Option.none();
  }

  const singlePin: Pin = { ...pin, bundle: { checks: [matched], open: [] } };
  const hydrated = await readPinScripts({ lockPath, pin: singlePin, root: pinRoot });
  const firstCheck = hydrated.pin.bundle.checks.at(0);

  return Option.fromNullishOr(firstCheck);
}

async function readLockCheck({
  checkId,
  lockPath,
  pinRoot,
}: ReadLockCheckInput): Promise<Option.Option<Check>> {
  const record = await readRecord({ path: lockPath, root: pinRoot });

  if (Option.isNone(record)) {
    return Option.none();
  }

  const decoded = decodePin(record.value.json);

  if (Option.isNone(decoded)) {
    return Option.none();
  }

  return await readMatchedCheck({ checkId, lockPath, pin: decoded.value, pinRoot });
}

async function readPinnedCheck({
  checkId,
  lockPath,
  pinRoot,
}: ReadPinnedCheckInput): Promise<Option.Option<Check>> {
  if (lockPath === undefined || lockPath.length === 0) {
    return Option.none();
  }

  const resolvedLockPath = lockPath;

  async function runRead(): Promise<Option.Option<Check>> {
    return await readLockCheck({ checkId, lockPath: resolvedLockPath, pinRoot });
  }

  const outcome = await classifyResult({ message: 'pin load failed', run: runRead });

  if (Result.isFailure(outcome)) {
    return Option.none();
  }

  return outcome.success;
}

function buildFallbackCheck({
  repro,
  target,
}: BuildFallbackInput): Result.Result<Check, string> {
  const runtime = decodeRuntime(repro.runtime);

  if (Option.isNone(runtime)) {
    const label = JSON.stringify(repro.runtime);

    return Result.fail(`unsupported replay runtime ${label}`);
  }

  const expected = decodeExpectedJson(repro.expected);

  if (Option.isNone(expected)) {
    return Result.fail('repro expected value is not valid JSON');
  }

  return Result.succeed({
    defect: '',
    expected: expected.value,
    good: '',
    id: target.check,
    promise: '',
    refactor: '',
    runtime: runtime.value,
    subject: target.subject ?? '',
    test: repro.code,
  });
}

/** Resolve one replay target from its staged pin lockfile or fallback witness repro. */
export async function resolveReplayCheck({
  root,
  state,
  target,
}: ResolveReplayCheckInput): Promise<Result.Result<Check, string>> {
  const pinRoot = selectPinRoot({ root, state, target });

  const pinned = await readPinnedCheck({
    checkId: target.check,
    lockPath: target.lockPath,
    pinRoot,
  });

  if (Option.isSome(pinned)) {
    return Result.succeed(pinned.value);
  }

  if (target.repro === undefined) {
    return Result.fail('pinned check was not found and no repro fallback was supplied');
  }

  return buildFallbackCheck({ repro: target.repro, target });
}
