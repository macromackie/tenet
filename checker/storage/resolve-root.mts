import { realpath } from 'node:fs/promises';
import { basename, dirname, join, resolve } from 'node:path';
import { Effect, Result } from 'effect';
import type { CheckerError } from '../checker-error.mts';
import { tryOperation } from '../try-operation.mts';

function readErrorCode(value: unknown): string {
  if (value instanceof Error && 'code' in value && typeof value.code === 'string') {
    return value.code;
  }

  return '';
}

async function readRealPath(path: string): Promise<Result.Result<string, CheckerError>> {
  async function run() {
    return await realpath(path);
  }

  const inspect = tryOperation({ message: 'resolve canonical root path', run });

  return await inspect.pipe(Effect.result, Effect.runPromise);
}

/** Resolve the canonical path of a root or state folder, including missing descendants. */
export async function resolveRoot(path: string): Promise<string> {
  const absolute = resolve(path);
  const resolved = await readRealPath(absolute);

  if (Result.isSuccess(resolved)) {
    return resolved.success;
  }

  const parent = dirname(absolute);

  if (readErrorCode(resolved.failure.cause) !== 'ENOENT' || parent === absolute) {
    throw resolved.failure;
  }

  const trusted = await resolveRoot(parent);
  const child = basename(absolute);

  return join(trusted, child);
}
