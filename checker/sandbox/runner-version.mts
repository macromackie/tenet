import type { Stats } from 'node:fs';
import { stat } from 'node:fs/promises';
import { Effect, Option } from 'effect';
import { readRustTool } from './rust-tool.mts';

const BWRAP = '/usr/bin/bwrap';
const PYTHON = '/usr/bin/python3';
const PRLIMIT = '/usr/bin/prlimit';
const RUNNER_REVISION = 3;

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

/** Fingerprint the runtimes that may execute a frozen observation. */
async function readRunnerVersion(): Promise<string> {
  const identities = Effect.forEach(
    [BWRAP, PYTHON, PRLIMIT, '/usr/bin/gcc', process.execPath],
    (path) => readBinaryIdentity(path),
    { concurrency: 4 },
  );

  const binaries = await Effect.runPromise(identities);
  const tool = Effect.tryPromise(async () => await readRustTool('-Vv'));
  const version = Effect.option(tool);
  const installed = await Effect.runPromise(version);
  const rust = Option.getOrNull(installed);

  return JSON.stringify([RUNNER_REVISION, process.version, process.platform, binaries, rust]);
}

export { readRunnerVersion as runnerVersion };
