import { Effect, type Result } from 'effect';
import type { CheckerError } from './checker-error.mts';
import { tryOperation } from './try-operation.mts';

type ClassifyResultInput<Value> = { readonly message: string; readonly run: () => Promise<Value> };

/** Retain the reason a bounded worker operation failed for the contract's evidence. */
export async function classifyResult<Value>(
  input: ClassifyResultInput<Value>,
): Promise<Result.Result<Value, CheckerError>> {
  const work = tryOperation(input);
  const classified = Effect.result(work);

  return await Effect.runPromise(classified);
}
