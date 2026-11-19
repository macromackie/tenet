import { CheckerError } from '../checker-error.mts';
import { readInventory } from '../discovery/index.mts';
import { replacePin, writeAtomic } from '../storage/index.mts';
import type { Pin } from '../types.d.mts';
import { evaluate } from './evidence.mts';
import type { Context } from './receipt.mts';
import { type Attempt, type Checked, selectPin } from './select-pin.mts';

type PublishInput = { ctx: Context; pin: Pin };

async function publish({ ctx, pin }: PublishInput): Promise<void> {
  const input = { path: ctx.lock, root: ctx.job.request.root, value: pin };

  if (ctx.pin === undefined) {
    await writeAtomic({ ...input, first: true });

    return;
  }

  await replacePin({ ...input, prior: ctx.prior });
}

type VerifyInput = { ctx: Context; attempt: Attempt };

/** Qualify and publish immutable observations only after the checkout is stable. */
export async function verify({ ctx, attempt }: VerifyInput): Promise<Checked> {
  const pin = await selectPin({ attempt, ctx });
  const rows = await evaluate({ evaluation: ctx.evaluation, pin });
  const after = await readInventory({ contract: ctx.job.contract, root: ctx.job.request.root });

  if (
    after.observed !== ctx.evaluation.current.observed ||
    after.shape !== ctx.evaluation.current.shape
  ) {
    throw new CheckerError({ message: 'source changed during the check. Run it again.' });
  }

  const results = rows.filter((row) => row.outcome !== 'unresolved');

  const cache =
    ctx.matching || (pin.bundle.checks.length > 0 && results.length === pin.bundle.checks.length);

  if (cache && !ctx.matching) {
    await publish({ ctx, pin });
  }

  return { cache, pin, rows };
}
