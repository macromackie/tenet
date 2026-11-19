import { Result } from 'effect';
import { classifyResult } from '../classify-result.mts';
import type { Finding, Pin } from '../types.d.mts';
import { buildOpenFinding } from './build-open-finding.mts';
import { readReason } from './read-reason.mts';
import type { Context } from './receipt.mts';
import { reportCoverage } from './report-coverage.mts';
import { saveReceipt } from './save-receipt.mts';
import type { Attempt, Checked, Preparation } from './select-pin.mts';
import { verify } from './verify.mts';

type MergeResultsInput = { readonly gaps: readonly Finding[]; readonly result: Checked };

function mergeResults({ gaps, result }: MergeResultsInput) {
  const rows = [...gaps, ...result.rows];

  if (result.cache) {
    return { pin: result.pin, rows };
  }

  return { rows };
}

async function collectFindings(
  options: Preparation,
): Promise<{ rows: readonly Finding[]; pin?: Pin }> {
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

/** Record the first provider charge alongside the contract’s coverage and executable cases. */
export async function measure(ctx: Context): Promise<readonly Finding[]> {
  const attempt: Attempt = { cost: null, requested: false };
  const result = await collectFindings({ attempt, ctx });

  await saveReceipt({ ctx, ...result });

  const first = result.rows.at(0);

  if (attempt.requested && first !== undefined) {
    Object.assign(first, { cost_usd: attempt.cost, requested: true });
  }

  return result.rows;
}
