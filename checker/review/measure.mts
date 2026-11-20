import { Result } from 'effect';
import { classifyResult } from '../classify-result.mts';
import { explainFindings } from '../fx/index.mts';
import type { Finding, Pin } from '../types.d.mts';
import { buildOpenFinding } from './build-open-finding.mts';
import { readReason } from './read-reason.mts';
import { type Context, saveReceipt } from './receipt/index.mts';
import { reportCoverage } from './report-coverage.mts';
import { type Checked, type Preparation, verify } from './verify.mts';

type Attempt = {
  cost: Finding['cost_usd'];
  hasUnknownCost: boolean;
  requested: boolean;
  requests: number;
};

type MergeResultsInput = { readonly gaps: readonly Finding[]; readonly result: Checked };

type AttachUsageInput = Readonly<{
  explanation: Attempt;
  generation: Attempt;
  rows: readonly Finding[];
}>;

type RecordCostInput = Readonly<{ attempt: Attempt; cost: Finding['cost_usd'] }>;

function createAttempt(): Attempt {
  return { cost: null, hasUnknownCost: false, requested: false, requests: 0 };
}

function recordAttemptCost({ attempt, cost }: RecordCostInput): void {
  const requests = attempt.requests + 1;

  if (cost === null) {
    Object.assign(attempt, { cost: null, hasUnknownCost: true, requested: true, requests });

    return;
  }

  if (attempt.hasUnknownCost) {
    Object.assign(attempt, { cost: null, requested: true, requests });

    return;
  }

  const nextCost = (attempt.cost ?? 0) + cost;

  Object.assign(attempt, { cost: nextCost, requested: true, requests });
}

function mergeResults({ gaps, result }: MergeResultsInput) {
  const rows = [...gaps, ...result.rows];

  if (result.cache) {
    return { pin: result.pin, rows };
  }

  return { rows };
}

async function collectFindings(
  options: Preparation,
): Promise<{ pin?: Pin; rows: readonly Finding[] }> {
  const { ctx } = options;

  const gaps = reportCoverage({
    contract: ctx.job.contract,
    omitted: ctx.evaluation.current.omitted,
  });

  async function runChecks() {
    return await verify(options);
  }

  const classified = await classifyResult({
    message: 'checker unavailable',
    run: runChecks,
  });

  if (Result.isFailure(classified)) {
    const message = readReason(classified.failure.cause);
    const warning = buildOpenFinding({ contract: ctx.job.contract.name, reason: message });

    return { rows: [...gaps, warning] };
  }

  return mergeResults({ gaps, result: classified.success });
}

function attachUsage({ explanation, generation, rows }: AttachUsageInput): void {
  const first = rows.at(0);

  if (first === undefined) {
    return;
  }

  if (generation.requested) {
    Object.assign(first, { cost_usd: generation.cost, requested: true });

    if (generation.requests > 1) {
      Object.assign(first, { generation_requests: generation.requests });
    }
  }

  if (explanation.requested) {
    Object.assign(first, {
      explanation_cost_usd: explanation.cost,
      explanation_requests: explanation.requests,
    });
  }
}

/** Record provider charges alongside the contract’s coverage and executable cases. */
export async function measure(ctx: Context): Promise<readonly Finding[]> {
  const generation = createAttempt();
  const explanation = createAttempt();

  function onGenerationRequest(cost: Finding['cost_usd']): void {
    recordAttemptCost({ attempt: generation, cost });
  }

  function onExplanationRequest(cost: Finding['cost_usd']): void {
    recordAttemptCost({ attempt: explanation, cost });
  }

  const result = await collectFindings({ ctx, onRequest: onGenerationRequest });

  const rows = await explainFindings({
    canSpendRequest: ctx.job.canSpendRequest,
    contract: ctx.job.contract,
    onRequest: onExplanationRequest,
    progressFile: ctx.job.request.progressFile,
    rows: result.rows,
  });

  await saveReceipt({ ctx, pin: result.pin, rows });
  attachUsage({ explanation, generation, rows });

  return rows;
}
