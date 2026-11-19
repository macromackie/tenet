import { Option } from 'effect';
import { hashSource, writeAtomic } from '../storage/index.mts';
import type { Finding, Pin, Receipt } from '../types.d.mts';
import type { Context } from './receipt.mts';

type SaveReceiptInput = {
  ctx: Context;
  rows: readonly Finding[];
  pin?: Pin;
};

/** Atomically record coverage, qualifications, and the pinned checker identity. */
export async function saveReceipt({ ctx, rows, pin }: SaveReceiptInput): Promise<void> {
  const {
    evaluation: { base, tools, current },
    job: { request },
  } = ctx;

  const qualifications = rows.filter((row) => row.outcome !== 'unresolved').map((row) => row.check);

  const candidate = Option.fromUndefinedOr(pin);
  const serialized = Option.map(candidate, (recipe) => JSON.stringify(recipe));
  const signature = Option.map(serialized, hashSource);

  const value: Receipt = {
    base,
    observed: current.observed,
    qualifications,
    rows,
    shape: current.shape,
    signature: Option.getOrUndefined(signature),
    tools,
  };

  await writeAtomic({ path: ctx.receipt, root: request.state, value });
}
