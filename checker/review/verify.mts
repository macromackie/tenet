import { CheckerError } from '../checker-error.mts';
import { readInventory } from '../discovery/index.mts';
import { reportProgress } from '../progress-log.mts';
import { replacePin, writeAtomic, writePinScripts } from '../storage/index.mts';
import type { Pin } from '../types.d.mts';
import { type Context, evaluate } from './receipt/index.mts';
import { type Checked, type Preparation, selectPin } from './select-pin.mts';

export type { Checked, Preparation } from './select-pin.mts';

type PublishInput = { ctx: Context; pin: Pin };

type VerifyInput = Preparation;

function readPublishRoot(ctx: Context): string {
  if (ctx.job.request.pinRoot !== undefined) {
    return ctx.job.request.pinRoot;
  }

  return ctx.job.request.root;
}

type WritePublishedLockInput = Readonly<{
  ctx: Context;
  diskPin: Pin;
  root: string;
}>;

async function writePublishedLock({ ctx, diskPin, root }: WritePublishedLockInput): Promise<void> {
  const input = { path: ctx.lock, root, value: diskPin };

  if (ctx.pin === undefined || ctx.legacyLock) {
    await writeAtomic({ ...input, first: !ctx.legacyLock });

    return;
  }

  await replacePin({ ...input, prior: ctx.prior });
}

async function publish({ ctx, pin }: PublishInput): Promise<Pin> {
  const root = readPublishRoot(ctx);

  const materialized = await writePinScripts({
    lockPath: ctx.lock,
    pin,
    priorPin: ctx.pin,
    root,
  });

  await writePublishedLock({ ctx, diskPin: materialized.diskPin, root });

  return materialized.hydratedPin;
}

type ResolvePublishedPinInput = Readonly<{
  cache: boolean;
  ctx: Context;
  pin: Pin;
}>;

async function resolvePublishedPin({ cache, ctx, pin }: ResolvePublishedPinInput): Promise<Pin> {
  if (cache && !ctx.matching) {
    return await publish({ ctx, pin });
  }

  return pin;
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
  const { extraUnresolved, pin, pinReused, prequalifiedIds } = await selectPin({ ctx, onRequest });

  await reportProgress({
    contract: ctx.job.contract.name,
    phase: 'replaying checks',
    progressFile: ctx.job.request.progressFile,
  });

  const observed = await evaluate({ evaluation: ctx.evaluation, pin, pinReused, prequalifiedIds });

  await assertStableSource(ctx);

  const rows = [...observed, ...extraUnresolved];
  const cache = ctx.matching || pin.bundle.checks.length > 0;
  const publishedPin = await resolvePublishedPin({ cache, ctx, pin });

  return { cache, pin: publishedPin, rows };
}
