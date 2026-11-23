import type { Stats } from 'node:fs';
import { stat } from 'node:fs/promises';
import { Effect, Option } from 'effect';
import { readGoVersion, readPythonTool, readRustTool } from './runtimes/index.mts';

const BWRAP = '/usr/bin/bwrap';
const PYTHON = '/usr/bin/python3';
const PRLIMIT = '/usr/bin/prlimit';
const SANDBOX_EXEC = '/usr/bin/sandbox-exec';
const RUNNER_REVISION = 6;

function readBinaryIdentity(path: string) {
  const read = Effect.tryPromise(async () => await stat(path));

  function buildMissingIdentity() {
    return [path, null];
  }

  function buildToolIdentity(info: Stats) {
    return [path, info.size, info.mtimeMs];
  }

  return Effect.match(read, { onFailure: buildMissingIdentity, onSuccess: buildToolIdentity });
}

async function readToolPaths(): Promise<readonly string[]> {
  if (process.platform === 'darwin') {
    const pythonProbe = Effect.tryPromise(async () => await readPythonTool());
    const pythonOption = Effect.option(pythonProbe);
    const pythonResult = await Effect.runPromise(pythonOption);
    const pythonPath = Option.getOrElse(pythonResult, () => PYTHON);

    return [SANDBOX_EXEC, pythonPath, process.execPath];
  }

  return [BWRAP, PYTHON, PRLIMIT, '/usr/bin/gcc', process.execPath];
}

/** Fingerprint the runtimes that may execute a frozen observation. */
async function readRunnerVersion(): Promise<string> {
  const paths = await readToolPaths();

  const identities = Effect.forEach(paths, (path) => readBinaryIdentity(path), {
    concurrency: 4,
  });

  const binaries = await Effect.runPromise(identities);
  const tool = Effect.tryPromise(async () => await readRustTool('-Vv'));
  const version = Effect.option(tool);
  const installed = await Effect.runPromise(version);
  const rust = Option.getOrNull(installed);
  const goInstalled = await readGoVersion();
  const go = Option.getOrNull(goInstalled);

  return JSON.stringify([RUNNER_REVISION, process.version, process.platform, binaries, rust, go]);
}

export { readRunnerVersion as runnerVersion };
