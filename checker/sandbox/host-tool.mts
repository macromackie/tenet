import { execFile, type ExecFileOptionsWithStringEncoding } from 'node:child_process';
import { promisify } from 'node:util';
import { Effect } from 'effect';
import { CheckerError } from '../checker-error.mts';

export type Execution = Readonly<{ stdout: string; stderr: string; status: number }>;

type CommandInput = Readonly<{
  file: string;
  args: readonly string[];
  options: ExecFileOptionsWithStringEncoding;
}>;

// oxlint-disable-next-line typescript/strict-void-return -- Node supplies a custom promise implementation for execFile, whose callback overload also returns the process handle.
const executeFile = promisify(execFile);

function readCompletedExit(error: unknown): Execution {
  const failure = new CheckerError({
    cause: error,
    message: 'the isolated tool could not run to completion',
  });

  if (!(error instanceof Error) || !('code' in error) || typeof error.code !== 'number') {
    throw failure;
  }

  if (!('stdout' in error) || typeof error.stdout !== 'string') {
    throw failure;
  }

  if (!('stderr' in error) || typeof error.stderr !== 'string') {
    throw failure;
  }

  return { status: error.code, stderr: error.stderr, stdout: error.stdout };
}

function createToolFailure(cause: unknown): CheckerError {
  return new CheckerError({ cause, message: 'external tool did not complete' });
}

function readFailedExecution(error: CheckerError): Execution {
  return readCompletedExit(error.cause);
}

function buildExecution(streams: Pick<Execution, 'stderr' | 'stdout'>): Execution {
  return { ...streams, status: 0 };
}

/** Launch a bounded host process and distinguish its exit status from a transport failure. */
export async function runFile({ file, args, options }: CommandInput): Promise<Execution> {
  async function run() {
    return await executeFile(file, args, options);
  }

  const launch = Effect.tryPromise({ catch: createToolFailure, try: run });

  const result = Effect.match(launch, {
    onFailure: readFailedExecution,
    onSuccess: buildExecution,
  });

  return await Effect.runPromise(result);
}
