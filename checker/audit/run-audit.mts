import { Result } from 'effect';
import type { CheckerError } from '../checker-error.mts';
import { classifyResult } from '../classify-result.mts';
import { reportProgress } from '../progress-log.mts';
import { run } from '../review/index.mts';
import { assertSandboxAvailable } from '../sandbox/index.mts';
import type { Finding } from '../types.d.mts';
import { buildModuleContracts, type SynthesisOutcome } from './synthesize.mts';
import type {
  AuditContractOutput,
  AuditModule,
  AuditModuleReport,
  AuditRequest,
  AuditResponse,
} from './wire.mts';

const REQUESTS_PER_CONTRACT = 3;

type RunAuditInput = AuditRequest;

type BudgetTracker = Readonly<{
  consume: (count: number) => void;
  readRemaining: () => number;
}>;

type AuditOneInput = Readonly<{
  budget: BudgetTracker;
  deep: boolean;
  fresh: boolean;
  module: AuditModule;
  progressFile?: string;
  root: string;
  runtimePaths: readonly string[];
  state: string;
}>;

type VerifyCandidatesInput = AuditOneInput & Readonly<{ synthesis: SynthesisOutcome }>;

type SumCostInput = Readonly<{
  first: Finding['cost_usd'];
  second: Finding['cost_usd'];
}>;

function addNullableCost({ first, second }: SumCostInput): Finding['cost_usd'] {
  if (first === null || second === null) {
    return null;
  }

  return first + second;
}

function readFindingRequests(findings: readonly Finding[]): number {
  let total = 0;

  for (const row of findings) {
    if (row.requested) {
      total += row.generation_requests ?? 1;
    }

    total += row.explanation_requests ?? 0;
  }

  return total;
}

function readFindingCost(findings: readonly Finding[]): Finding['cost_usd'] {
  let total: Finding['cost_usd'] = 0;

  for (const row of findings) {
    if (row.requested) {
      total = addNullableCost({ first: total, second: row.cost_usd });
    }

    if (row.explanation_requests !== undefined && row.explanation_requests > 0) {
      total = addNullableCost({ first: total, second: row.explanation_cost_usd ?? null });
    }
  }

  return total;
}

function createBudget(limit: number): BudgetTracker {
  let left = Math.max(0, limit);

  function consume(count: number): void {
    left = Math.max(0, left - count);
  }

  function readRemaining(): number {
    return left;
  }

  return { consume, readRemaining };
}

type FailedModuleInput = Readonly<{
  cause: unknown;
  name: string;
}>;

function readFailureReason(error: unknown): string {
  if (error instanceof Error && error.message.length > 0) {
    return error.message;
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

  const reviewRequests = readFindingRequests(findings);
  const reviewCost = readFindingCost(findings);

  budget.consume(reviewRequests);

  const contractOutputs = mapContractOutputs(synthesis);
  const costUsd = addNullableCost({ first: synthesis.costUsd, second: reviewCost });

  return {
    contracts: contractOutputs,
    costUsd,
    findings,
    name: module.name,
    requests: synthesis.requests + reviewRequests,
  };
}

async function buildSafely(
  input: AuditOneInput,
): Promise<Result.Result<SynthesisOutcome, CheckerError>> {
  const { deep, module, progressFile, root } = input;

  await reportProgress({ contract: module.name, phase: 'synthesizing contracts', progressFile });

  async function runSynthesis(): Promise<SynthesisOutcome> {
    return await buildModuleContracts({ deep, module, root });
  }

  return await classifyResult({ message: 'module synthesis failed', run: runSynthesis });
}

function buildEmptyModuleReport(name: string): AuditModuleReport {
  return { contracts: [], costUsd: 0, findings: [], name, requests: 0 };
}

async function auditSingleModule(input: AuditOneInput): Promise<AuditModuleReport> {
  if (input.budget.readRemaining() <= 0) {
    return buildEmptyModuleReport(input.module.name);
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

function summarizeAuditReports(reports: readonly AuditModuleReport[]): AuditResponse {
  let totalCost: Finding['cost_usd'] = 0;
  let totalRequests = 0;

  for (const report of reports) {
    totalCost = addNullableCost({ first: totalCost, second: report.costUsd });
    totalRequests += report.requests;
  }

  return { costUsd: totalCost, modules: reports, requests: totalRequests };
}

/** Synthesize and falsify behavioral contracts across a bounded list of repository modules. */
export async function runAudit({
  deep = false,
  fresh = false,
  maxRequests,
  modules,
  progressFile,
  root,
  runtimePaths = [],
  state,
}: RunAuditInput): Promise<AuditResponse> {
  await assertSandboxAvailable();

  const budget = createBudget(maxRequests);
  const reports: AuditModuleReport[] = [];

  for (const module of modules) {
    if (budget.readRemaining() <= 0) {
      break;
    }

    // Sequential module auditing keeps shared request budgets exact and predictable.
    // oxlint-disable-next-line no-await-in-loop
    const report = await auditSingleModule({
      budget,
      deep,
      fresh,
      module,
      progressFile,
      root,
      runtimePaths,
      state,
    });

    reports.push(report);
  }

  return summarizeAuditReports(reports);
}
