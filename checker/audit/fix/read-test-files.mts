import { Option, Result } from 'effect';
import { classifyResult } from '../../classify-result.mts';
import { readRegular } from '../../storage/index.mts';

const MAX_TEST_FILE_BYTES = 12_000;
const MAX_TEST_TOTAL_BYTES = 36_000;

type ReadTestFilesInput = Readonly<{
  paths: readonly string[];
  root: string;
}>;

type ReadOneTestInput = Readonly<{
  limit: number;
  path: string;
  root: string;
}>;

type TestPoolState = Readonly<{
  entries: readonly (readonly [string, string])[];
  remaining: number;
}>;

type AdvanceTestInput = Readonly<{
  path: string;
  root: string;
  state: TestPoolState;
}>;

async function readSingleTestFile({
  limit,
  path,
  root,
}: ReadOneTestInput): Promise<Option.Option<string>> {
  async function readText(): Promise<string> {
    return await readRegular({ limit, path, root, truncate: true });
  }

  const outcome = await classifyResult({
    message: 'bounded fix test read unavailable',
    run: readText,
  });

  return Result.getSuccess(outcome);
}

async function advanceTestPool({ path, root, state }: AdvanceTestInput): Promise<TestPoolState> {
  const limit = Math.min(state.remaining, MAX_TEST_FILE_BYTES);
  const text = await readSingleTestFile({ limit, path, root });

  if (Option.isNone(text)) {
    return state;
  }

  return {
    entries: [...state.entries, [path, text.value]],
    remaining: state.remaining - Buffer.byteLength(text.value),
  };
}

/** Load bounded repository test files from disk for repair prompt context without staging them in the sandbox. */
export async function readBoundedTestFiles({
  paths,
  root,
}: ReadTestFilesInput): Promise<Readonly<Record<string, string>>> {
  let state: TestPoolState = { entries: [], remaining: MAX_TEST_TOTAL_BYTES };

  for (const path of paths) {
    if (state.remaining <= 0) {
      break;
    }

    // Sequential bounded reads enforce the shared test-file byte ceiling deterministically.
    // oxlint-disable-next-line no-await-in-loop
    state = await advanceTestPool({ path, root, state });
  }

  // ast-grep-ignore: object-from-entries-typescript -- Prompt payload serializes bounded test files as a path-keyed JSON object.
  return Object.fromEntries(state.entries);
}
