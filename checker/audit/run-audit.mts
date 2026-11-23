import { reportProgress } from '../progress-log.mts';
import { assertSandboxAvailable } from '../sandbox/index.mts';
import type { Finding } from '../types.d.mts';
import { mergeModuleReports } from './merge-report.mts';
import { rejectUnverifiedWitnesses } from './reject-unverified.mts';
import { type BudgetTracker, type RoundInput, runModuleRound } from './run-round.mts';
import { reviewFailingWitnesses } from './skeptic.mts';
import type { PriorRoundFeedback } from './synthesize.mts';
import type { AuditModule, AuditModuleReport, AuditRequest, AuditResponse } from './wire.mts';

const SHARPEN_MIN_BUDGET = 5;
const MULTI_FILE_SHARD_MIN = 3;
const MAX_PRIOR_GUARDS = 16;
const MAX_PRIOR_WITNESSES = 8;
const MAX_PRIOR_UNRESOLVED = 8;

type RunAuditInput = AuditRequest;

type SumCostInput = Readonly<{
  first: Finding['cost_usd'];
  second: Finding['cost_usd'];
}>;

type ShouldSharpenInput = Readonly<{
  budget: BudgetTracker;
  deep: boolean;
  feedback: PriorRoundFeedback;
  module: AuditModule;
  report: AuditModuleReport;
}>;

type ApplySkepticInput = Readonly<{
  budget: BudgetTracker;
  deep: boolean;
  module: AuditModule;
  progressFile?: string;
  report: AuditModuleReport;
  root: string;
}>;

type SummarizeOutcomeInput = Readonly<{
  findings: readonly Finding[];
  limit: number;
  outcome: Finding['outcome'];
}>;

function addCost({ first, second }: SumCostInput): Finding['cost_usd'] {
  if (first === null || second === null) {
    return null;
  }

  return first + second;
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

function summarizeByOutcome({
  findings,
  limit,
  outcome,
}: SummarizeOutcomeInput): readonly string[] {
  return findings
    .filter((row) => row.outcome === outcome)
    .slice(0, limit)
    .map((row) => `${row.subject ?? row.contract}/${row.check}: ${row.message}`);
}

function readPriorFeedback(findings: readonly Finding[]): PriorRoundFeedback {
  return {
    guards: summarizeByOutcome({ findings, limit: MAX_PRIOR_GUARDS, outcome: 'passed' }),
    unresolved: summarizeByOutcome({
      findings,
      limit: MAX_PRIOR_UNRESOLVED,
      outcome: 'unresolved',
    }),
    witnesses: summarizeByOutcome({ findings, limit: MAX_PRIOR_WITNESSES, outcome: 'failed' }),
  };
}

function hasFailingWitness(findings: readonly Finding[]): boolean {
  return findings.some((row) => row.outcome === 'failed');
}

function countWitnessSubjects(findings: readonly Finding[]): number {
  const witnessSubjects = new Set<string>();

  for (const row of findings) {
    if (row.outcome === 'failed' && row.subject !== null) {
      witnessSubjects.add(row.subject);
    }
  }

  return witnessSubjects.size;
}

function hasActionableSignal(feedback: PriorRoundFeedback): boolean {
  return feedback.guards.length > 0 || feedback.unresolved.length > 0;
}

function shouldSharpen({ budget, deep, feedback, module, report }: ShouldSharpenInput): boolean {
  if (!deep || !hasActionableSignal(feedback) || budget.readRemaining() < SHARPEN_MIN_BUDGET) {
    return false;
  }

  const coveredSubjects = countWitnessSubjects(report.findings);

  if (coveredSubjects === 0) {
    return true;
  }

  return module.files.length >= MULTI_FILE_SHARD_MIN && coveredSubjects <= 1;
}

async function applySkepticFilter({
  budget,
  deep,
  module,
  progressFile,
  report,
  root,
}: ApplySkepticInput): Promise<AuditModuleReport> {
  if (!hasFailingWitness(report.findings)) {
    return report;
  }

  if (budget.readRemaining() <= 0) {
    return { ...report, findings: rejectUnverifiedWitnesses(report.findings) };
  }

  await reportProgress({ contract: module.name, phase: 'reviewing witnesses', progressFile });

  const reviewed = await reviewFailingWitnesses({
    deep,
    findings: report.findings,
    module,
    root,
  });

  budget.consume(reviewed.requests);

  return {
    ...report,
    costUsd: addCost({ first: report.costUsd, second: reviewed.costUsd }),
    findings: reviewed.findings,
    requests: report.requests + reviewed.requests,
  };
}

async function runWithOptionalSharpening(input: RoundInput): Promise<AuditModuleReport> {
  const first = await runModuleRound(input);
  const feedback = readPriorFeedback(first.findings);

  if (
    !shouldSharpen({
      budget: input.budget,
      deep: input.deep,
      feedback,
      module: input.module,
      report: first,
    })
  ) {
    return first;
  }

  const extra = await runModuleRound({ ...input, priorFeedback: feedback });

  return mergeModuleReports({ base: first, extra });
}

async function auditSingleModule(input: RoundInput): Promise<AuditModuleReport> {
  const report = await runWithOptionalSharpening(input);

  return await applySkepticFilter({
    budget: input.budget,
    deep: input.deep,
    module: input.module,
    progressFile: input.progressFile,
    report,
    root: input.root,
  });
}

function summarizeAuditReports(reports: readonly AuditModuleReport[]): AuditResponse {
  let totalCost: Finding['cost_usd'] = 0;
  let totalRequests = 0;

  for (const report of reports) {
    totalCost = addCost({ first: totalCost, second: report.costUsd });
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
