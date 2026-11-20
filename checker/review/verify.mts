import { CheckerError } from '../checker-error.mts';
import { readInventory } from '../discovery/index.mts';
import { reportProgress } from '../progress-log.mts';
import { replacePin, writeAtomic } from '../storage/index.mts';
import type { Pin } from '../types.d.mts';
import { type Context, evaluate } from './receipt/index.mts';
import { type Checked, type Preparation, selectPin } from './select-pin.mts';

export type { Checked, Preparation } from './select-pin.mts';

type PublishInput = { ctx: Context; pin: Pin };

type VerifyInput = Preparation;

async function publish({ ctx, pin }: PublishInput): Promise<void> {
  const input = { path: ctx.lock, root: ctx.job.request.root, value: pin };

  if (ctx.pin === undefined) {
    await writeAtomic({ ...input, first: true });

    return;
  }

  await replacePin({ ...input, prior: ctx.prior });
}

async function assertStableSource(ctx: Context): Promise<void> {
  const after = await readInventory({ contract: ctx.job.contract, root: ctx.job.request.root });

  if (
    after.observed !== ctx.evaluation.current.observed ||
    after.shape !== ctx.evaluation.current.shape
  ) {
    throw new CheckerError({ message: 'source changed during the check. Run it again.' });
  }
}

/** Qualify and publish immutable observations only after the checkout is stable. */
export async function verify({ ctx, onRequest }: VerifyInput): Promise<Checked> {
  const { extraUnresolved, pin, prequalifiedIds } = await selectPin({ ctx, onRequest });

  await reportProgress({
    contract: ctx.job.contract.name,
    phase: 'replaying checks',
    progressFile: ctx.job.request.progressFile,
  });

  const observed = await evaluate({ evaluation: ctx.evaluation, pin, prequalifiedIds });

  await assertStableSource(ctx);

  const rows = [...observed, ...extraUnresolved];
  const cache = ctx.matching || pin.bundle.checks.length > 0;

  if (cache && !ctx.matching) {
    await publish({ ctx, pin });
  }

  return { cache, pin, rows };
}
