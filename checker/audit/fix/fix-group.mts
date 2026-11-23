import type { FixPromptWitness } from '../../fx/index.mts';
import type { Finding } from '../../types.d.mts';
import { replayTarget } from '../replay-target.mts';
import type { AuditReplayResult } from '../replay-wire.mts';
import type { BudgetTracker } from '../run-round.mts';
import { buildBoundedSources } from './bound-fix-sources.mts';
import { buildFixOutcome } from './build-fix-outcome.mts';
import type { AuditFixGroup, AuditFixOutcome, AuditFixOutcomeStatus } from './fix-wire.mts';
import { type AttemptContext, type AttemptOutcome, runFixAttempt } from './run-fix-attempt.mts';
import type { PendingWitness, PrecheckResult } from './verify-fix-patch.mts';

const MAX_FIX_ATTEMPTS = 3;
const MAX_SOURCE_FILE_BYTES = 65_536;
const MAX_SOURCE_TOTAL_BYTES = 204_800;
const MAX_RELATED_FILE_BYTES = 12_000;
const MAX_RELATED_TOTAL_BYTES = 48_000;

type PrecheckInput = Readonly<{
  currentFiles: ReadonlyMap<string, string>;
  group: AuditFixGroup;
  root: string;
  state: string;
}>;

type RepairGroupInput = PrecheckInput &
  Readonly<{
    budget: BudgetTracker;
    deep: boolean;
  }>;

type RepairGroupResult = Readonly<{
  changedPaths: readonly string[];
  costUsd: Finding['cost_usd'];
  files: ReadonlyMap<string, string>;
  outcomes: readonly AuditFixOutcome[];
  requests: number;
}>;

type PromptWitnessInput = PendingWitness;

type RetryLoopInput = RepairGroupInput & Readonly<{ precheck: PrecheckResult }>;

type CostPairInput = Readonly<{
  base: Finding['cost_usd'];
  delta: Finding['cost_usd'];
}>;

type AttemptState = Readonly<{
  costUsd: Finding['cost_usd'];
  priorAttemptError?: string;
  requests: number;
  resolved?: RepairGroupResult;
}>;

type AdvanceAttemptInput = Readonly<{
  alreadyPassing: readonly AuditFixOutcome[];
  baseContext: Omit<AttemptContext, 'priorAttemptError'>;
  budget: BudgetTracker;
  state: AttemptState;
}>;

type BuildStateInput = Readonly<{
  alreadyPassing: readonly AuditFixOutcome[];
  result: AttemptOutcome;
  state: AttemptState;
}>;

function combineCost({ base, delta }: CostPairInput): Finding['cost_usd'] {
  if (base === null || delta === null) {
    return null;
  }

  return base + delta;
}

function mapFailureStatus(result: AuditReplayResult): AuditFixOutcomeStatus {
  if (result.outcome === 'failed') {
    return 'failed';
  }

  return 'unresolved';
}

function buildPromptWitness({ initial, witness }: PromptWitnessInput): FixPromptWitness {
  return {
    check: witness.check,
    contract: witness.contract,
    expected: initial.expected,
    message: witness.message,
    observed: initial.observed,
    repro: witness.repro,
    subject: witness.subject,
  };
}

async function checkInitialWitnesses({
  currentFiles,
  group,
  root,
  state,
}: PrecheckInput): Promise<PrecheckResult> {
  const alreadyPassing: AuditFixOutcome[] = [];
  const pending: PendingWitness[] = [];

  for (const witness of group.witnesses) {
    // Sequential pre-check avoids unnecessary sandbox contention before LLM calls.
    // oxlint-disable-next-line no-await-in-loop
    const result = await replayTarget({ files: currentFiles, root, state, target: witness });

    if (result.outcome === 'passed') {
      const outcome = buildFixOutcome({ result, status: 'already_passing' });

      alreadyPassing.push(outcome);
    } else {
      pending.push({ initial: result, witness });
    }
  }

  return { alreadyPassing, pending };
}

function buildFailedOutcomes(pending: readonly PendingWitness[]): readonly AuditFixOutcome[] {
  return pending.map((item) => {
    const status = mapFailureStatus(item.initial);

    return buildFixOutcome({ result: item.initial, status });
  });
}

function buildAttemptContext(input: RetryLoopInput): Omit<AttemptContext, 'priorAttemptError'> {
  const sources = buildBoundedSources({
    files: input.currentFiles,
    paths: input.group.files,
    perFileBytes: MAX_SOURCE_FILE_BYTES,
    totalBytes: MAX_SOURCE_TOTAL_BYTES,
  });

  const relatedSources = buildBoundedSources({
    files: input.currentFiles,
    paths: input.group.relatedFiles ?? [],
    perFileBytes: MAX_RELATED_FILE_BYTES,
    totalBytes: MAX_RELATED_TOTAL_BYTES,
  });

  const promptWitnesses = input.precheck.pending.map((item) => buildPromptWitness(item));

  return {
    currentFiles: input.currentFiles,
    deep: input.deep,
    group: input.group,
    pending: input.precheck.pending,
    promptWitnesses,
    relatedSources,
    root: input.root,
    sources,
    state: input.state,
  };
}

function buildAttemptState({ alreadyPassing, result, state }: BuildStateInput): AttemptState {
  const costUsd = combineCost({ base: state.costUsd, delta: result.costUsd });
  const requests = state.requests + result.requests;

  if (result.files !== undefined && result.outcomes !== undefined) {
    const resolved: RepairGroupResult = {
      changedPaths: result.changedPaths,
      costUsd,
      files: result.files,
      outcomes: [...alreadyPassing, ...result.outcomes],
      requests,
    };

    return { costUsd, requests, resolved };
  }

  return { costUsd, priorAttemptError: result.error, requests };
}

async function advanceRepairAttempt({
  alreadyPassing,
  baseContext,
  budget,
  state,
}: AdvanceAttemptInput): Promise<AttemptState> {
  const result = await runFixAttempt({
    ...baseContext,
    priorAttemptError: state.priorAttemptError,
  });

  budget.consume(result.requests);

  return buildAttemptState({ alreadyPassing, result, state });
}

async function runRepairLoop(input: RetryLoopInput): Promise<RepairGroupResult> {
  const baseContext = buildAttemptContext(input);
  let state: AttemptState = { costUsd: 0, requests: 0 };

  for (let attempt = 0; attempt < MAX_FIX_ATTEMPTS; attempt += 1) {
    if (input.budget.readRemaining() <= 0 || state.resolved !== undefined) {
      break;
    }

    // Sequential retry attempts depend on prior patch and sandbox verification feedback.
    // oxlint-disable-next-line no-await-in-loop
    state = await advanceRepairAttempt({
      alreadyPassing: input.precheck.alreadyPassing,
      baseContext,
      budget: input.budget,
      state,
    });
  }

  if (state.resolved !== undefined) {
    return state.resolved;
  }

  const failedOutcomes = buildFailedOutcomes(input.precheck.pending);

  return {
    changedPaths: [],
    costUsd: state.costUsd,
    files: input.currentFiles,
    outcomes: [...input.precheck.alreadyPassing, ...failedOutcomes],
    requests: state.requests,
  };
}

/** Pre-check one group's witnesses and run bounded sandbox-verified repair attempts if needed. */
export async function repairAuditGroup(input: RepairGroupInput): Promise<RepairGroupResult> {
  const precheck = await checkInitialWitnesses({
    currentFiles: input.currentFiles,
    group: input.group,
    root: input.root,
    state: input.state,
  });

  if (precheck.pending.length === 0) {
    return {
      changedPaths: [],
      costUsd: 0,
      files: input.currentFiles,
      outcomes: precheck.alreadyPassing,
      requests: 0,
    };
  }

  return await runRepairLoop({ ...input, precheck });
}
