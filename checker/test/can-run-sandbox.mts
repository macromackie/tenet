import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { Result } from 'effect';
import { classifyResult } from '../classify-result.mts';

// The Node callback overload also exposes its ChildProcess.
// oxlint-disable-next-line typescript/strict-void-return
const execute = promisify(execFile);

async function runSandbox() {
  return await execute('/usr/bin/bwrap', [
    '--unshare-all',
    '--ro-bind',
    '/',
    '/',
    '--',
    '/usr/bin/true',
  ]);
}

/** Detect support for isolated behavior tests before exercising one. */
export async function canRun(): Promise<boolean> {
  if (process.platform !== 'linux') {
    return false;
  }

  const result = await classifyResult({
    message: 'sandbox unavailable',
    run: runSandbox,
  });

  return Result.isSuccess(result);
}
