import { Option, Result, Schema } from 'effect';
import type { CheckerError } from '../../checker-error.mts';
import { classifyResult } from '../../classify-result.mts';
import { observe, validate } from '../../sandbox/index.mts';
import type { Check, Finding, Inventory } from '../../types.d.mts';
import { buildFinding } from '../build-finding.mts';
import { buildOpenFinding } from '../build-open-finding.mts';

const MAX_CHECKS = 12;
const MAX_OPEN_LENGTH = 400;
const RawChecks = Schema.Array(Schema.Unknown);
const OpenItems = Schema.Array(Schema.String);
const RawBundle = Schema.Struct({ checks: RawChecks, open: OpenItems });
const decodeRawBundle = Schema.decodeUnknownOption(RawBundle, { onExcessProperty: 'error' });

export type CandidateEvaluation = Readonly<{
  errors: readonly string[];
  extraUnresolved: readonly Finding[];
  open: readonly string[];
  qualified: readonly Check[];
}>;

type QualifyBundleInput = Readonly<{
  contractName: string;
  current: Inventory;
  value: unknown;
}>;

type SingleOutcome =
  | Readonly<{ check: Check; kind: 'qualified' }>
  | Readonly<{ error: string; finding: Finding; kind: 'failed' }>;

type CheckLabelInput = Readonly<{ candidate: unknown; index: number }>;

type ParseSingleInput = Readonly<{ files: Inventory['files']; raw: unknown }>;

type QualifySingleInput = Readonly<{ check: Check; files: Inventory['runtimeFiles'] }>;

type FailCandidateInput = Readonly<{
  check: string;
  contractName: string;
  detail: string;
  subject?: string;
}>;

type RunCheckQualificationInput = Readonly<{
  candidate: Check;
  contractName: string;
  files: Inventory['runtimeFiles'];
}>;

type SingleInput = Readonly<{
  contractName: string;
  current: Inventory;
  index: number;
  raw: unknown;
  seenIds: ReadonlySet<string>;
}>;

type ListState = Readonly<{
  errors: readonly string[];
  extraUnresolved: readonly Finding[];
  qualified: readonly Check[];
  seenIds: ReadonlySet<string>;
}>;

type RecordOutcomeInput = Readonly<{ outcome: SingleOutcome; state: ListState }>;

type EmptyErrorInput = Readonly<{ contractName: string; message: string }>;

type QualifyListInput = Readonly<{
  contractName: string;
  current: Inventory;
  open: readonly string[];
  rawChecks: readonly unknown[];
}>;

function readCandidateError(error: unknown): string {
  if (error instanceof Error) {
    return error.message;
  }

  return 'candidate check could not be qualified';
}

function readCheckLabel({ candidate, index }: CheckLabelInput): string {
  if (candidate !== null && typeof candidate === 'object' && 'id' in candidate) {
    const { id } = candidate;

    if (typeof id === 'string' && id.trim().length > 0) {
      return id.trim();
    }
  }

  return `candidate-${String(index + 1)}`;
}

function buildEmptyError({ contractName, message }: EmptyErrorInput): CandidateEvaluation {
  const warning = buildOpenFinding({ contract: contractName, reason: message });

  return { errors: [message], extraUnresolved: [warning], open: [], qualified: [] };
}

function isValidOpenItem(item: string): boolean {
  const trimmed = item.trim();

  return trimmed.length > 0 && trimmed.length <= MAX_OPEN_LENGTH;
}

type FindBundleProblemInput = Readonly<{ checks: readonly unknown[]; open: readonly string[] }>;

function findBundleProblem({ checks, open }: FindBundleProblemInput): Option.Option<string> {
  if (checks.length > MAX_CHECKS || open.length > MAX_CHECKS) {
    return Option.some('checker plan exceeds the number of allowed checks');
  }

  if (checks.length === 0 && open.length === 0) {
    return Option.some('checker plan returned neither checks nor open items');
  }

  if (!open.every((item) => isValidOpenItem(item))) {
    return Option.some(
      'checker plan open items must be non-empty strings of at most 400 characters',
    );
  }

  return Option.none();
}

function parseSingle({ files, raw }: ParseSingleInput): Result.Result<Check, string> {
  function runValidate() {
    return validate({ check: raw, files });
  }

  return Result.try({ catch: readCandidateError, try: runValidate });
}

async function qualifySingle({
  check,
  files,
}: QualifySingleInput): Promise<Result.Result<Check, CheckerError>> {
  async function runQualification() {
    await observe({ check, files, qualify: true, skipActual: true });

    return check;
  }

  return await classifyResult({ message: 'qualification failed', run: runQualification });
}

function failCandidate({
  check,
  contractName,
  detail,
  subject,
}: FailCandidateInput): SingleOutcome {
  const finding = buildFinding({
    check,
    contract: contractName,
    message: detail,
    outcome: 'unresolved',
    subject,
  });

  return { error: `${check}: ${detail}`, finding, kind: 'failed' };
}

async function runCheckQualification({
  candidate,
  contractName,
  files,
}: RunCheckQualificationInput): Promise<SingleOutcome> {
  const result = await qualifySingle({ check: candidate, files });

  if (Result.isSuccess(result)) {
    return { check: result.success, kind: 'qualified' };
  }

  const detail = readCandidateError(result.failure.cause);

  return failCandidate({
    check: candidate.id,
    contractName,
    detail,
    subject: candidate.subject,
  });
}

async function checkCandidate({
  contractName,
  current,
  index,
  raw,
  seenIds,
}: SingleInput): Promise<SingleOutcome> {
  const label = readCheckLabel({ candidate: raw, index });
  const parsed = parseSingle({ files: current.files, raw });

  if (Result.isFailure(parsed)) {
    return failCandidate({ check: label, contractName, detail: parsed.failure });
  }

  const candidate = parsed.success;

  if (seenIds.has(candidate.id)) {
    return failCandidate({
      check: candidate.id,
      contractName,
      detail: 'duplicate check identifier',
      subject: candidate.subject,
    });
  }

  return await runCheckQualification({ candidate, contractName, files: current.runtimeFiles });
}

function recordOutcome({ outcome, state }: RecordOutcomeInput): ListState {
  if (outcome.kind === 'qualified') {
    const nextIds = new Set([...state.seenIds, outcome.check.id]);

    return {
      ...state,
      qualified: [...state.qualified, outcome.check],
      seenIds: nextIds,
    };
  }

  return {
    ...state,
    errors: [...state.errors, outcome.error],
    extraUnresolved: [...state.extraUnresolved, outcome.finding],
  };
}

async function qualifyList({
  contractName,
  current,
  open,
  rawChecks,
}: QualifyListInput): Promise<CandidateEvaluation> {
  let state: ListState = {
    errors: [],
    extraUnresolved: [],
    qualified: [],
    seenIds: new Set<string>(),
  };

  for (const [index, raw] of rawChecks.entries()) {
    // Qualify each candidate sequentially so every sandbox receives its full resource allowance.
    // oxlint-disable-next-line no-await-in-loop
    const outcome = await checkCandidate({
      contractName,
      current,
      index,
      raw,
      seenIds: state.seenIds,
    });

    state = recordOutcome({ outcome, state });
  }

  return {
    errors: state.errors,
    extraUnresolved: state.extraUnresolved,
    open,
    qualified: state.qualified,
  };
}

/** Validate and qualify generated candidate checks on controls only, never inspecting actual code. */
export async function qualifyBundle({
  contractName,
  current,
  value,
}: QualifyBundleInput): Promise<CandidateEvaluation> {
  const decoded = decodeRawBundle(value);

  if (Option.isNone(decoded)) {
    return buildEmptyError({
      contractName,
      message:
        'the checker plan has invalid top-level fields; return { checks: [...], open: [...] }',
    });
  }

  const problem = findBundleProblem(decoded.value);

  if (Option.isSome(problem)) {
    return buildEmptyError({ contractName, message: problem.value });
  }

  const { checks: rawChecks, open } = decoded.value;

  return await qualifyList({ contractName, current, open, rawChecks });
}
