import { access, realpath } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { env as hostEnvironment } from 'node:process';
import { Result } from 'effect';
import { CheckerError } from '../../checker-error.mts';
import { classifyResult } from '../../classify-result.mts';
import { runFile } from '../host-tool.mts';

const SYSTEM_PYTHON = '/usr/bin/python3';
const TOOL_TIMEOUT_MS = 20_000;
const OUTPUT_BYTES = 8_192;

async function probeHostPython(): Promise<string> {
  const env = {
    HOME: hostEnvironment.HOME,
    PATH: hostEnvironment.PATH,
  };

  const cwd = tmpdir();

  const result = await runFile({
    args: ['-c', 'import os, sys; print(os.path.realpath(sys.executable))'],
    file: 'python3',
    options: {
      cwd,
      encoding: 'utf-8',
      env,
      killSignal: 'SIGKILL',
      maxBuffer: OUTPUT_BYTES,
      timeout: TOOL_TIMEOUT_MS,
    },
  });

  if (result.status !== 0) {
    throw new CheckerError({ message: 'python3 is unavailable' });
  }

  const executable = result.stdout.trim();

  if (executable.length === 0) {
    throw new CheckerError({ message: 'python3 returned an empty executable path' });
  }

  return await realpath(executable);
}

/** Resolve the concrete Python 3 binary behind any shim without forwarding provider credentials. */
export async function readPythonTool(): Promise<string> {
  const probed = await classifyResult({
    message: 'python3 probe failed',
    run: probeHostPython,
  });

  if (Result.isSuccess(probed)) {
    return probed.success;
  }

  await access(SYSTEM_PYTHON);

  return await realpath(SYSTEM_PYTHON);
}
