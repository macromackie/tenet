import { execFile } from 'node:child_process';
import { env } from 'node:process';
import { promisify } from 'node:util';
import { CheckerError } from '../checker-error.mts';

// The overloaded Node callback returns a ChildProcess alongside the promisified result.
// oxlint-disable-next-line typescript/strict-void-return
const execute = promisify(execFile);
const OBJECT_ID = /^(?:[a-f0-9]{40}|[a-f0-9]{64})$/v;
const FILE_LIMIT = 524_288;
const GIT_TIMEOUT = 3_000;

type ReadBaseInputsInput = {
  base: string;
  files: ReadonlyMap<string, string>;
  root: string;
};

type RunGitInput = { root: string; args: readonly string[] };

async function runGit({ root, args }: RunGitInput): Promise<string> {
  const result = await execute(
    'git',
    ['--no-replace-objects', '--literal-pathspecs', '-C', root, ...args],
    {
      env: { GIT_CONFIG_NOSYSTEM: '1', PATH: env.PATH },
      maxBuffer: FILE_LIMIT,
      timeout: GIT_TIMEOUT,
    },
  );

  return result.stdout;
}

/** Rebuild the observation's inputs from the pinned PR base, including removed or new subjects. */
export async function readBaseInputs({
  base,
  files,
  root,
}: ReadBaseInputsInput): Promise<Map<string, string>> {
  if (!OBJECT_ID.test(base)) {
    throw new CheckerError({ message: 'no pinned PR base is available.' });
  }

  const priorFiles = new Map<string, string>();

  for (const path of files.keys()) {
    // Bound the number of simultaneous Git readers and baseline bytes.
    // oxlint-disable-next-line no-await-in-loop
    const entry = await runGit({ args: ['ls-tree', '-z', base, '--', path], root });

    if (entry.length === 0) {
      continue;
    }

    // oxlint-disable-next-line no-await-in-loop
    const content = await runGit({ args: ['cat-file', 'blob', `${base}:${path}`], root });

    priorFiles.set(path, content);
  }

  return priorFiles;
}
