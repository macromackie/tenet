import { Effect, type Option } from 'effect';
import { readGoTool } from './read-go-tool.mts';

/** Read the installed Go version string when available on PATH. */
export async function readGoVersion(): Promise<Option.Option<string>> {
  const probe = Effect.tryPromise(async () => await readGoTool(['version']));
  const optional = Effect.option(probe);

  return await Effect.runPromise(optional);
}
