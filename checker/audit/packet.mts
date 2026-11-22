import { Option, Result } from 'effect';
import { classifyResult } from '../classify-result.mts';
import { readRegular } from '../storage/index.mts';
import type { AuditFileWindow, AuditModule } from './wire.mts';

const MAX_PACKET_BYTES = 204_800;
const MAX_FILE_BYTES = 65_536;
const MAX_WINDOWED_FILE_BYTES = 524_288;
const WINDOW_HEADER_LINES = 40;
const MAX_DOC_BYTES = 16_384;
const MAX_TOTAL_DOC_BYTES = 32_768;
const MAX_RELATED_BYTES = 12_000;
const MAX_TOTAL_RELATED_BYTES = 48_000;
const MAX_TEST_BYTES = 12_000;
const MAX_TOTAL_TEST_BYTES = 36_000;

type ModulePacket = Readonly<{
  docs: Readonly<Record<string, string>>;
  files: readonly string[];
  relatedSources: Readonly<Record<string, string>>;
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
  window?: AuditFileWindow;
}>;

type ReadGroupInput = Readonly<{
  fileWindows?: ReadonlyMap<string, AuditFileWindow>;
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
  window?: AuditFileWindow;
}>;

type SliceWindowInput = Readonly<{
  content: string;
  window: AuditFileWindow;
}>;

function buildWindowLookup(
  windows?: readonly AuditFileWindow[],
): ReadonlyMap<string, AuditFileWindow> {
  const fileWindows = new Map<string, AuditFileWindow>();

  if (windows === undefined) {
    return fileWindows;
  }

  for (const item of windows) {
    fileWindows.set(item.path, item);
  }

  return fileWindows;
}

type FormatWindowInput = Readonly<{
  body: string;
  lines: readonly string[];
  marker: string;
  startIndex: number;
}>;

function formatWindowText({ body, lines, marker, startIndex }: FormatWindowInput): string {
  if (startIndex <= 0) {
    return `${marker}\n${body}\n`;
  }

  const headerEnd = Math.min(WINDOW_HEADER_LINES, startIndex);
  const header = lines.slice(0, headerEnd).join('\n');

  return `${header}\n${marker}\n${body}\n`;
}

function sliceFileWindow({ content, window }: SliceWindowInput): string {
  const lines = content.split('\n');
  const startIndex = Math.max(0, window.startLine - 1);
  const clampedEnd = Math.max(window.endLine, window.startLine);
  const endIndex = Math.min(lines.length, clampedEnd);
  const body = lines.slice(startIndex, endIndex).join('\n');
  const marker = `[window lines ${String(window.startLine)}-${String(endIndex)} of ${String(lines.length)}]`;

  return formatWindowText({ body, lines, marker, startIndex });
}

async function readEntryContent({ limit, path, root, window }: BoundedFileInput): Promise<string> {
  if (window === undefined) {
    return await readRegular({ limit, path, root, truncate: true });
  }

  const full = await readRegular({
    limit: MAX_WINDOWED_FILE_BYTES,
    path,
    root,
    truncate: true,
  });

  return sliceFileWindow({ content: full, window });
}

async function readBoundedEntry(input: BoundedFileInput): Promise<Option.Option<LoadedEntry>> {
  async function readSource() {
    return await readEntryContent(input);
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

  return Option.some({ bytes, content: content.value, path: input.path });
}

async function advanceGroup({
  path,
  perFileBytes,
  root,
  state,
  window,
}: AdvanceGroupInput): Promise<GroupState> {
  const limit = Math.min(state.remaining, perFileBytes);
  const loaded = await readBoundedEntry({ limit, path, root, window });

  if (Option.isNone(loaded)) {
    return state;
  }

  return {
    entries: [...state.entries, [loaded.value.path, loaded.value.content]],
    remaining: state.remaining - loaded.value.bytes,
  };
}

async function readGroup({
  fileWindows,
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

    const window = fileWindows?.get(path);

    // Sequential reads keep total bytes strictly bounded.
    // oxlint-disable-next-line no-await-in-loop
    state = await advanceGroup({ path, perFileBytes, root, state, window });
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
  const fileWindows = buildWindowLookup(module.windows);

  const sources = await readGroup({
    fileWindows,
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

  const relatedSources = await readGroup({
    limitBytes: MAX_TOTAL_RELATED_BYTES,
    paths: module.relatedFiles ?? [],
    perFileBytes: MAX_RELATED_BYTES,
    root,
  });

  const tests = await readOptionalTests({ includeTests, module, root });

  return { docs, files: Object.keys(sources), relatedSources, sources, tests };
}
