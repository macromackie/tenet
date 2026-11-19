import { Option } from 'effect';
import { hashSource } from '../storage/index.mts';
import type { Finding } from '../types.d.mts';
import { buildOpenFinding } from './build-open-finding.mts';
import type { Context } from './receipt.mts';

function isCurrentReceipt(input: Context): boolean {
  const { evaluation } = input;
  const { base, current, saved, tools } = evaluation;

  if (saved === undefined || saved.base !== base) {
    return false;
  }

  return (
    saved.tools === tools && saved.observed === current.observed && saved.shape === current.shape
  );
}

function isCurrentPin(ctx: Context) {
  const candidate = Option.fromUndefinedOr(ctx.pin);
  const serialized = Option.map(candidate, (pin) => JSON.stringify(pin));
  const signature = Option.map(serialized, hashSource);

  return ctx.evaluation.saved?.signature === Option.getOrUndefined(signature);
}

function isReusable(ctx: Context): boolean {
  const { saved } = ctx.evaluation;

  if (!isCurrentReceipt(ctx) || saved === undefined) {
    return false;
  }

  if (!isCurrentPin(ctx)) {
    return false;
  }

  if (saved.rows.some((row) => row.outcome === 'unresolved')) {
    return ctx.job.request.inspect === true;
  }

  if (ctx.pin === undefined) {
    return false;
  }

  return ctx.matching;
}

/** Reuse only fresh evidence bound to the same observed files and tools. */
export function readCached(ctx: Context): Option.Option<readonly Finding[]> {
  const { saved } = ctx.evaluation;

  if (saved !== undefined && isReusable(ctx)) {
    const cached = saved.rows.map((row) => ({
      ...row,
      cost_usd: null,
      requested: false,
      reused: true,
    }));

    return Option.some(cached);
  }

  if (ctx.job.request.inspect !== true) {
    return Option.none();
  }

  if (ctx.pin === undefined && saved === undefined) {
    return Option.some([]);
  }

  const rows = [
    buildOpenFinding({
      contract: ctx.job.contract.name,
      reason: 'Replay the checker on the current inputs.',
    }),
  ];

  return Option.some(rows);
}
