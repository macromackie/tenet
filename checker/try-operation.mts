import { Effect } from 'effect';
import { CheckerError } from './checker-error.mts';

type TryOperationInput<Value> = {
  readonly message: string;
  readonly run: (signal: AbortSignal) => Promise<Value>;
};

/** Preserve the rejected foreign operation as the cause of a stable checker diagnostic. */
export function tryOperation<Value>(
  input: TryOperationInput<Value>,
): Effect.Effect<Value, CheckerError> {
  function fail(cause: unknown) {
    return new CheckerError({ cause, message: input.message });
  }

  return Effect.tryPromise({ catch: fail, try: input.run });
}
