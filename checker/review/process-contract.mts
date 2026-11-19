import { Option, Result } from 'effect';
import { classifyResult } from '../classify-result.mts';
import type { Finding } from '../types.d.mts';
import { buildOpenFinding } from './build-open-finding.mts';
import { measure } from './measure.mts';
import { readCached } from './read-cached.mts';
import { readReason } from './read-reason.mts';
import { type Job, readContext } from './receipt.mts';

/** Collect qualified observations or a reason that this contract still needs review. */
export async function processContract(job: Job): Promise<readonly Finding[]> {
  async function assessScope() {
    const ctx = await readContext(job);
    const cached = readCached(ctx);

    if (Option.isSome(cached)) {
      return cached.value;
    }

    return await measure(ctx);
  }

  const classified = await classifyResult({
    message: 'could not read contract scope',
    run: assessScope,
  });

  if (Result.isSuccess(classified)) {
    return classified.success;
  }

  const reason = readReason(classified.failure.cause);

  return [buildOpenFinding({ contract: job.contract.name, reason })];
}
