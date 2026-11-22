import { Option, Result } from 'effect';
import { classifyResult } from '../classify-result.mts';
import { readRegular } from '../storage/index.mts';
import type { AuditModule } from './wire.mts';

const MAX_PACKET_BYTES = 204_800;
const MAX_FILE_BYTES = 65_536;
const MAX_DOC_BYTES = 16_384;
const MAX_TOTAL_DOC_BYTES = 32_768;
const MAX_TEST_BYTES = 8_000;
const MAX_TOTAL_TEST_BYTES = 24_000;

type ModulePacket = Readonly<{
  docs: Readonly<Record<string, string>>;
  files: readonly string[];
  sources: Readonly<Record<string, string>>;
  tests: Readonly<Record<string, string>>;
}>;

type ReadPacketInput = Readonly<{
  includeTests?: boolean;
  module: AuditModule;
  root: string;
}>;

type BoundedFileInput = Readonly<{
  limit: number;
  path: string;
  root: string;
}>;

type ReadGroupInput = Readonly<{
  limitBytes: number;
  paths: readonly string[];
  perFileBytes: number;
  root: string;
}>;

type LoadedEntry = Readonly<{
  bytes: number;
  content: string;
  path: string;
}>;

type GroupState = Readonly<{
  entries: readonly [string, string][];
  remaining: number;
}>;

type AdvanceGroupInput = Readonly<{
  path: string;
  perFileBytes: number;
  root: string;
  state: GroupState;
}>;

async function readBoundedEntry({
  limit,
  path,
  root,
}: BoundedFileInput): Promise<Option.Option<LoadedEntry>> {
  async function readSource() {
    return await readRegular({ limit, path, root });
  }

  const outcome = await classifyResult({
    message: 'bounded audit read unavailable',
    run: readSource,
  });

  const content = Result.getSuccess(outcome);

  if (Option.isNone(content)) {
    return Option.none();
  }

  const bytes = Buffer.byteLength(content.value);

  return Option.some({ bytes, content: content.value, path });
}

async function advanceGroup({
  path,
  perFileBytes,
  root,
  state,
}: AdvanceGroupInput): Promise<GroupState> {
  const limit = Math.min(state.remaining, perFileBytes);
  const loaded = await readBoundedEntry({ limit, path, root });

  if (Option.isNone(loaded)) {
    return state;
  }

  return {
    entries: [...state.entries, [loaded.value.path, loaded.value.content]],
    remaining: state.remaining - loaded.value.bytes,
  };
}

async function readGroup({
  limitBytes,
  paths,
  perFileBytes,
  root,
}: ReadGroupInput): Promise<Readonly<Record<string, string>>> {
  let state: GroupState = { entries: [], remaining: limitBytes };

  for (const path of paths) {
    if (state.remaining <= 0) {
      break;
    }

    // Sequential reads keep total bytes strictly bounded.
    // oxlint-disable-next-line no-await-in-loop
    state = await advanceGroup({ path, perFileBytes, root, state });
  }

  // ast-grep-ignore: object-from-entries-typescript -- Prompt payload serializes bounded files as a path-keyed JSON object.
  return Object.fromEntries(state.entries);
}

async function readOptionalTests({
  includeTests = false,
  module,
  root,
}: ReadPacketInput): Promise<Readonly<Record<string, string>>> {
  if (!includeTests) {
    return {};
  }

  return await readGroup({
    limitBytes: MAX_TOTAL_TEST_BYTES,
    paths: module.testFiles ?? [],
    perFileBytes: MAX_TEST_BYTES,
    root,
  });
}

/** Load bounded module source files, manifests, docs, and optional related tests into one packet. */
export async function readModulePacket({
  includeTests = false,
  module,
  root,
}: ReadPacketInput): Promise<ModulePacket> {
  const sources = await readGroup({
    limitBytes: MAX_PACKET_BYTES,
    paths: [...module.manifests, ...module.files],
    perFileBytes: MAX_FILE_BYTES,
    root,
  });

  const docs = await readGroup({
    limitBytes: MAX_TOTAL_DOC_BYTES,
    paths: module.docs,
    perFileBytes: MAX_DOC_BYTES,
    root,
  });

  const tests = await readOptionalTests({ includeTests, module, root });

  return { docs, files: Object.keys(sources), sources, tests };
}
