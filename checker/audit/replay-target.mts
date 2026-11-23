import { Option, Result } from 'effect';
import { classifyResult } from '../classify-result.mts';
import { observe } from '../sandbox/index.mts';
import { readRegular } from '../storage/index.mts';
import type { Check } from '../types.d.mts';
import type { AuditReplayResult, ReplayTarget } from './replay-wire.mts';
import { resolveReplayCheck } from './resolve-replay-check.mts';

const MAX_JSON_PREVIEW = 512;
const MAX_MESSAGE_CHARS = 512;
const MAX_ON_DEMAND_BYTES = 524_288;
const ELLIPSIS = '...';

type ReplayTargetInput = Readonly<{
  files: ReadonlyMap<string, string>;
  root: string;
  state: string;
  target: ReplayTarget;
}>;

type ResolveSubjectInput = Readonly<{
  checkSubject: string;
  targetSubject?: string;
}>;

type BuildUnresolvedInput = Readonly<{
  expected?: string;
  message: string;
  subject?: string;
  target: ReplayTarget;
}>;

type EnsureSubjectInput = Readonly<{
  files: ReadonlyMap<string, string>;
  root: string;
  subject: string;
}>;

type FormatMessageInput = Readonly<{
  expected: string;
  observed: string;
  passed: boolean;
  promise: string;
}>;

type FormatOutcomeInput = Readonly<{
  check: Check;
  passed: boolean;
  subject?: string;
  target: ReplayTarget;
  value: unknown;
}>;

type ExecuteReplayInput = Readonly<{
  check: Check;
  files: ReadonlyMap<string, string>;
  root: string;
  target: ReplayTarget;
}>;

function cleanMessage(text: string): string {
  return (
    text
      // Strip terminal controls from untrusted sandbox observations.
      // oxlint-disable-next-line no-control-regex
      .replaceAll(/[\u0000-\u001F\u007F-\u009F\u2028\u2029\u202A-\u202E\u2066-\u2069]/gv, ' ')
      .trim()
      .slice(0, MAX_MESSAGE_CHARS)
  );
}

function formatCompactJson(value: unknown): string {
  if (value === undefined) {
    return 'undefined';
  }

  const serialized = JSON.stringify(value);

  if (serialized.length <= MAX_JSON_PREVIEW) {
    return serialized;
  }

  const clipped = serialized.slice(0, MAX_JSON_PREVIEW - ELLIPSIS.length);

  return `${clipped}${ELLIPSIS}`;
}

function readErrorText(cause: unknown): string {
  if (cause instanceof Error && cause.message.length > 0) {
    return cleanMessage(cause.message);
  }

  return 'could not replay check inside the sandbox';
}

function resolveSubject({
  checkSubject,
  targetSubject,
}: ResolveSubjectInput): Option.Option<string> {
  if (checkSubject.length > 0) {
    return Option.some(checkSubject);
  }

  if (targetSubject !== undefined && targetSubject.length > 0) {
    return Option.some(targetSubject);
  }

  return Option.none();
}

function buildUnresolvedResult({
  expected,
  message,
  subject,
  target,
}: BuildUnresolvedInput): AuditReplayResult {
  const sanitized = cleanMessage(message);

  return {
    check: target.check,
    contract: target.contract,
    expected,
    message: sanitized,
    outcome: 'unresolved',
    subject,
  };
}

async function ensureSubjectLoaded({
  files,
  root,
  subject,
}: EnsureSubjectInput): Promise<ReadonlyMap<string, string>> {
  if (subject.length === 0 || files.has(subject)) {
    return files;
  }

  async function readSubject(): Promise<string> {
    return await readRegular({ limit: MAX_ON_DEMAND_BYTES, path: subject, root });
  }

  const loaded = await classifyResult({ message: 'subject file unavailable', run: readSubject });

  if (Result.isFailure(loaded)) {
    return files;
  }

  const entries: readonly [string, string][] = [...files, [subject, loaded.success]];

  return new Map<string, string>(entries);
}

function formatObservedMessage({
  expected,
  observed,
  passed,
  promise,
}: FormatMessageInput): string {
  if (!passed) {
    return cleanMessage(`expected ${expected}, observed ${observed}`);
  }

  if (promise.length > 0) {
    return cleanMessage(`passed: ${promise}`);
  }

  return cleanMessage(`passed (observed ${observed})`);
}

function readObservedOutcome(passed: boolean): 'passed' | 'failed' {
  if (passed) {
    return 'passed';
  }

  return 'failed';
}

function buildObservedResult({
  check,
  passed,
  subject,
  target,
  value,
}: FormatOutcomeInput): AuditReplayResult {
  const expected = formatCompactJson(check.expected);
  const observed = formatCompactJson(value);
  const outcome = readObservedOutcome(passed);
  const message = formatObservedMessage({ expected, observed, passed, promise: check.promise });

  return {
    check: target.check,
    contract: target.contract,
    expected,
    message,
    observed,
    outcome,
    subject,
  };
}

async function executeReplay({
  check,
  files,
  root,
  target,
}: ExecuteReplayInput): Promise<AuditReplayResult> {
  const subjectOption = resolveSubject({
    checkSubject: check.subject,
    targetSubject: target.subject,
  });

  const subject = Option.getOrUndefined(subjectOption);
  const replayFiles = await ensureSubjectLoaded({ files, root, subject: check.subject });

  async function runObservation() {
    return await observe({ check, files: replayFiles, projectRoot: root, qualify: false });
  }

  const observed = await classifyResult({ message: 'sandbox replay failed', run: runObservation });

  if (Result.isFailure(observed)) {
    const expected = formatCompactJson(check.expected);
    const message = readErrorText(observed.failure.cause);

    return buildUnresolvedResult({ expected, message, subject, target });
  }

  return buildObservedResult({
    check,
    passed: observed.success.passed,
    subject,
    target,
    value: observed.success.value,
  });
}

/** Replay one pinned or repro-backed check directly inside the OS sandbox with zero LLM calls. */
export async function replayTarget({
  files,
  root,
  state,
  target,
}: ReplayTargetInput): Promise<AuditReplayResult> {
  const resolved = await resolveReplayCheck({ root, state, target });

  if (Result.isFailure(resolved)) {
    return buildUnresolvedResult({ message: resolved.failure, subject: target.subject, target });
  }

  return await executeReplay({ check: resolved.success, files, root, target });
}
