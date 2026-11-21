import { Result } from 'effect';
import type { CheckerError } from '../../checker-error.mts';
import { classifyResult } from '../../classify-result.mts';
import { observe, validate } from '../../sandbox/index.mts';
import type { Check, Finding, Inventory } from '../../types.d.mts';
import { buildFinding } from '../build-finding.mts';
import { buildOpenFinding } from '../build-open-finding.mts';
import { decodePlannerBundle } from './decode-bundle.mts';

export type CandidateEvaluation = Readonly<{
  errors: readonly string[];
  extraUnresolved: readonly Finding[];
  open: readonly string[];
  qualified: readonly Check[];
}>;

type QualifyBundleInput = Readonly<{
  contractName: string;
  current: Inventory;
  projectRoot?: string;
  value: unknown;
}>;

type SingleOutcome =
  | Readonly<{ check: Check; kind: 'qualified' }>
  | Readonly<{ error: string; finding: Finding; kind: 'failed' }>;

type CheckLabelInput = Readonly<{ candidate: unknown; index: number }>;

type ParseSingleInput = Readonly<{ files: Inventory['files']; raw: unknown }>;

type QualifySingleInput = Readonly<{
  check: Check;
  files: Inventory['runtimeFiles'];
  projectRoot?: string;
}>;

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
  projectRoot?: string;
}>;

type SingleInput = Readonly<{
  contractName: string;
  current: Inventory;
  index: number;
  projectRoot?: string;
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
  projectRoot?: string;
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

function parseSingle({ files, raw }: ParseSingleInput): Result.Result<Check, string> {
  function runValidate() {
    return validate({ check: raw, files });
  }

  return Result.try({ catch: readCandidateError, try: runValidate });
}

async function qualifySingle({
  check,
  files,
  projectRoot,
}: QualifySingleInput): Promise<Result.Result<Check, CheckerError>> {
  async function runQualification() {
    await observe({ check, files, projectRoot, qualify: true, skipActual: true });

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
  projectRoot,
}: RunCheckQualificationInput): Promise<SingleOutcome> {
  const result = await qualifySingle({ check: candidate, files, projectRoot });

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
  projectRoot,
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

  return await runCheckQualification({
    candidate,
    contractName,
    files: current.runtimeFiles,
    projectRoot,
  });
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
  projectRoot,
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
      projectRoot,
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
  projectRoot,
  value,
}: QualifyBundleInput): Promise<CandidateEvaluation> {
  const decoded = decodePlannerBundle(value);

  if (Result.isFailure(decoded)) {
    return buildEmptyError({ contractName, message: decoded.failure });
  }

  return await qualifyList({ contractName, current, projectRoot, ...decoded.success });
}
