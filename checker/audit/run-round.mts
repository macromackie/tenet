import { Result } from 'effect';
import type { CheckerError } from '../checker-error.mts';
import { classifyResult } from '../classify-result.mts';
import { reportProgress } from '../progress-log.mts';
import { run } from '../review/index.mts';
import type { Finding } from '../types.d.mts';
import { buildModuleContracts, type SynthesisOutcome } from './synthesize.mts';
import type { AuditContractOutput, AuditModule, AuditModuleReport } from './wire.mts';

const REQUESTS_PER_CONTRACT = 3;

export type BudgetTracker = Readonly<{
  consume: (count: number) => void;
  readRemaining: () => number;
}>;

export type RoundInput = Readonly<{
  budget: BudgetTracker;
  deep: boolean;
  fresh: boolean;
  module: AuditModule;
  priorGuards?: readonly string[];
  progressFile?: string;
  root: string;
  runtimePaths: readonly string[];
  state: string;
}>;

type VerifyCandidatesInput = RoundInput & Readonly<{ synthesis: SynthesisOutcome }>;

type SumCostInput = Readonly<{
  first: Finding['cost_usd'];
  second: Finding['cost_usd'];
}>;

type FailedModuleInput = Readonly<{
  cause: unknown;
  name: string;
}>;

function addCost({ first, second }: SumCostInput): Finding['cost_usd'] {
  if (first === null || second === null) {
    return null;
  }

  return first + second;
}

function readFindingTotals(findings: readonly Finding[]) {
  let cost: Finding['cost_usd'] = 0;
  let requests = 0;

  for (const row of findings) {
    if (row.requested) {
      requests += row.generation_requests ?? 1;
      cost = addCost({ first: cost, second: row.cost_usd });
    }

    if (row.explanation_requests !== undefined && row.explanation_requests > 0) {
      requests += row.explanation_requests;
      cost = addCost({ first: cost, second: row.explanation_cost_usd ?? null });
    }
  }

  return { cost, requests };
}

function readFailureReason(cause: unknown): string {
  if (cause instanceof Error && cause.message.length > 0) {
    return cause.message;
  }

  return 'could not synthesize module contracts';
}

function buildFailedModuleReport({ cause, name }: FailedModuleInput): AuditModuleReport {
  const message = readFailureReason(cause);

  const finding: Finding = {
    check: 'synthesis',
    contract: name,
    cost_usd: null,
    message,
    outcome: 'unresolved',
    requested: false,
    reused: false,
    subject: null,
  };

  return { contracts: [], costUsd: null, findings: [finding], name, requests: 0 };
}

function mapContractOutputs(synthesis: SynthesisOutcome): readonly AuditContractOutput[] {
  return synthesis.candidates.map((item) => ({
    document: item.document,
    lockPath: item.lockPath,
    message: item.message,
    name: item.name,
    paths: item.paths,
  }));
}

async function verifyModuleCandidates({
  budget,
  fresh,
  module,
  progressFile,
  root,
  runtimePaths,
  state,
  synthesis,
}: VerifyCandidatesInput): Promise<AuditModuleReport> {
  const leftBudget = budget.readRemaining();
  const reviewBudget = Math.min(leftBudget, synthesis.candidates.length * REQUESTS_PER_CONTRACT);
  const contracts = synthesis.candidates.map((item) => ({ ...item.contract, runtimePaths }));

  const findings = await run({
    contracts,
    fresh,
    maxGenerations: reviewBudget,
    pinRoot: state,
    progressFile,
    root,
    state,
  });

  const totals = readFindingTotals(findings);

  budget.consume(totals.requests);

  return {
    contracts: mapContractOutputs(synthesis),
    costUsd: addCost({ first: synthesis.costUsd, second: totals.cost }),
    findings,
    name: module.name,
    requests: synthesis.requests + totals.requests,
  };
}

function readSynthesisPhase(priorGuards: readonly string[]): string {
  if (priorGuards.length > 0) {
    return 'sharpening hypotheses';
  }

  return 'synthesizing contracts';
}

async function buildSafely(
  input: RoundInput,
): Promise<Result.Result<SynthesisOutcome, CheckerError>> {
  const { deep, module, priorGuards = [], progressFile, root } = input;
  const phase = readSynthesisPhase(priorGuards);

  await reportProgress({ contract: module.name, phase, progressFile });

  async function runSynthesis(): Promise<SynthesisOutcome> {
    return await buildModuleContracts({ deep, module, priorGuards, root });
  }

  return await classifyResult({ message: 'module synthesis failed', run: runSynthesis });
}

/** Synthesize and verify one round of behavioral contracts for a single repository module. */
export async function runModuleRound(input: RoundInput): Promise<AuditModuleReport> {
  if (input.budget.readRemaining() <= 0) {
    return { contracts: [], costUsd: 0, findings: [], name: input.module.name, requests: 0 };
  }

  const synthResult = await buildSafely(input);

  if (Result.isFailure(synthResult)) {
    return buildFailedModuleReport({ cause: synthResult.failure.cause, name: input.module.name });
  }

  const synthesis = synthResult.success;

  input.budget.consume(synthesis.requests);

  if (synthesis.candidates.length === 0) {
    return {
      contracts: [],
      costUsd: synthesis.costUsd,
      findings: [],
      name: input.module.name,
      requests: synthesis.requests,
    };
  }

  return await verifyModuleCandidates({ ...input, synthesis });
}
