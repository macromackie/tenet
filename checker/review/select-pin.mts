import { CheckerError } from '../checker-error.mts';
import { generate } from '../generation.mts';
import { parseBundle } from '../schema.mts';
import type { Finding, Pin } from '../types.d.mts';
import type { Context } from './receipt.mts';
import { updatePin } from './update-pin.mts';

export type Attempt = {
  cost: Finding['cost_usd'];
  requested: boolean;
};

export type Checked = {
  cache: boolean;
  pin: Pin;
  rows: readonly Finding[];
};

export type Preparation = { ctx: Context; attempt: Attempt };

async function generatePin(input: Preparation): Promise<Pin> {
  const { ctx, attempt } = input;

  const {
    job: { contract, canSpendRequest },
    evaluation: { current },
  } = ctx;

  if (!canSpendRequest()) {
    throw new CheckerError({ message: 'no saved checker. Generation needs a model request.' });
  }

  function onRequest(cost: Finding['cost_usd']): void {
    Object.assign(attempt, { cost, requested: true });
  }

  const value = await generate({ contract, inventory: current, onRequest });

  return {
    bundle: parseBundle({ files: current.files, value }),
    contractHash: contract.hash,
    protocol: 1,
    shape: current.shape,
  };
}

/** Discover a replacement when the contract or its observed repository surface changes. */
export async function selectPin(input: Preparation): Promise<Pin> {
  const { ctx } = input;

  if (ctx.evaluation.current.files.size === 0) {
    throw new CheckerError({ message: 'no supported source was available for a behavior check.' });
  }

  if (ctx.pin !== undefined && ctx.matching) {
    return ctx.pin;
  }

  const generated = await generatePin(input);

  if (ctx.pin === undefined) {
    return generated;
  }

  return updatePin({ files: ctx.evaluation.current.runtimeFiles, next: generated, prior: ctx.pin });
}
