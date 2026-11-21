import { readFile } from 'node:fs/promises';
import { Option, Result } from 'effect';
import { classifyResult } from '../classify-result.mts';
import { resolveWorkerAsset, runFxJson } from '../fx/index.mts';
import { readRegular } from '../storage/index.mts';
import type { Finding } from '../types.d.mts';
import { parseAuditCandidates, type ParsedCandidate } from './parse-candidates.mts';
import type { AuditModule } from './wire.mts';

const MAX_PACKET_BYTES = 204_800;
const MAX_FILE_BYTES = 65_536;
const MAX_DOC_BYTES = 16_384;
const MAX_TOTAL_DOC_BYTES = 32_768;

export type SynthesisOutcome = Readonly<{
  candidates: readonly ParsedCandidate[];
  costUsd: Finding['cost_usd'];
  requests: number;
}>;

type SynthesizeModuleInput = Readonly<{
  deep: boolean;
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

type CostMeter = Readonly<{
  onRequest: (cost: Finding['cost_usd']) => void;
  readTotals: () => Readonly<{ costUsd: Finding['cost_usd']; requests: number }>;
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

function createCostMeter(): CostMeter {
  let costUsd: Finding['cost_usd'] = 0;
  let requests = 0;

  function onRequest(cost: Finding['cost_usd']): void {
    requests += 1;

    if (cost === null || costUsd === null) {
      costUsd = null;

      return;
    }

    costUsd += cost;
  }

  function readTotals(): Readonly<{ costUsd: Finding['cost_usd']; requests: number }> {
    return { costUsd, requests };
  }

  return { onRequest, readTotals };
}

async function readAuditPrompt(): Promise<string> {
  const promptPath = await resolveWorkerAsset('audit-prompt.md');

  return await readFile(promptPath, 'utf-8');
}

async function buildModulePayload({ deep, module, root }: SynthesizeModuleInput): Promise<string> {
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

  const files = Object.keys(sources);

  return JSON.stringify({ deep, docs, files, module: module.name, sources });
}

/** Synthesize 2-5 testable behavioral contract candidates for one repository module. */
export async function buildModuleContracts({
  deep,
  module,
  root,
}: SynthesizeModuleInput): Promise<SynthesisOutcome> {
  const instructions = await readAuditPrompt();
  const source = await buildModulePayload({ deep, module, root });
  const meter = createCostMeter();
  const raw = await runFxJson({ instructions, onRequest: meter.onRequest, source });
  const allowedPaths = [...module.manifests, ...module.files];

  const candidates = parseAuditCandidates({
    allowedPaths,
    manifestPaths: module.manifests,
    moduleName: module.name,
    raw,
  });

  const totals = meter.readTotals();

  return { candidates, ...totals };
}
