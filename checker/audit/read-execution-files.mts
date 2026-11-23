import { Option, Result } from 'effect';
import { classifyResult } from '../classify-result.mts';
import { readRegular } from '../storage/index.mts';

const MAX_EXECUTION_BYTES = 16_777_216;
const MAX_EXECUTION_FILES = 1_024;
const MAX_SOURCE_BYTES = 524_288;

type ReadExecutionFilesInput = Readonly<{
  primaryPaths: readonly string[];
  root: string;
  secondaryPaths?: readonly string[];
}>;

type ReadBoundedFileInput = Readonly<{
  limit: number;
  path: string;
  root: string;
}>;

type UniquePathsInput = Readonly<{
  primaryPaths: readonly string[];
  secondaryPaths?: readonly string[];
}>;

type PoolState = Readonly<{
  entries: readonly (readonly [string, string])[];
  remaining: number;
}>;

type AdvancePoolInput = Readonly<{
  path: string;
  root: string;
  state: PoolState;
}>;

function collectUniquePaths({
  primaryPaths,
  secondaryPaths = [],
}: UniquePathsInput): readonly string[] {
  const uniquePaths = new Set<string>();

  for (const path of [...primaryPaths, ...secondaryPaths]) {
    if (path.length > 0) {
      uniquePaths.add(path);
    }
  }

  return [...uniquePaths];
}

async function readBoundedFile(input: ReadBoundedFileInput): Promise<Option.Option<string>> {
  async function runRead(): Promise<string> {
    return await readRegular(input);
  }

  const outcome = await classifyResult({
    message: 'execution file unavailable',
    run: runRead,
  });

  return Result.getSuccess(outcome);
}

async function advancePool({ path, root, state }: AdvancePoolInput): Promise<PoolState> {
  const limit = Math.min(state.remaining, MAX_SOURCE_BYTES);
  const content = await readBoundedFile({ limit, path, root });

  if (Option.isNone(content)) {
    return state;
  }

  const bytes = Buffer.byteLength(content.value);

  return {
    entries: [...state.entries, [path, content.value]],
    remaining: state.remaining - bytes,
  };
}

/** Read bounded working-tree execution files into a path-to-source map. */
export async function readExecutionFiles({
  primaryPaths,
  root,
  secondaryPaths = [],
}: ReadExecutionFilesInput): Promise<Map<string, string>> {
  const orderedPaths = collectUniquePaths({ primaryPaths, secondaryPaths });
  let state: PoolState = { entries: [], remaining: MAX_EXECUTION_BYTES };

  for (const path of orderedPaths) {
    if (state.entries.length >= MAX_EXECUTION_FILES || state.remaining <= 0) {
      break;
    }

    // Sequential reads enforce the shared execution byte budget deterministically.
    // oxlint-disable-next-line no-await-in-loop
    state = await advancePool({ path, root, state });
  }

  return new Map<string, string>(state.entries);
}
