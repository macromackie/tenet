import { Result } from 'effect';
import { classifyResult } from '../classify-result.mts';
import { assertSandboxAvailable } from '../sandbox/index.mts';

/** Detect support for isolated behavior tests before exercising one. */
export async function canRun(): Promise<boolean> {
  const result = await classifyResult({
    message: 'sandbox unavailable',
    run: assertSandboxAvailable,
  });

  return Result.isSuccess(result);
}
