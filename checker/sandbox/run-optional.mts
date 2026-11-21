import { Effect, type Option } from 'effect';

/** Run an async filesystem probe and return None instead of throwing when paths are absent. */
export async function runOptional<Value>(
  run: () => Promise<Value>,
): Promise<Option.Option<Value>> {
  const attempt = Effect.tryPromise(run);
  const optional = Effect.option(attempt);

  return await Effect.runPromise(optional);
}
