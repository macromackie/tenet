import { readFile } from 'node:fs/promises';
import { resolveWorkerAsset, runFxJson } from '../fx/index.mts';
import type { Finding } from '../types.d.mts';
import { selectAuditTier } from './model-tier.mts';
import { readModulePacket } from './packet.mts';
import { parseAuditCandidates, type ParsedCandidate } from './parse-candidates.mts';
import type { AuditModule } from './wire.mts';

export type PriorRoundFeedback = Readonly<{
  guards: readonly string[];
  unresolved: readonly string[];
  witnesses: readonly string[];
}>;

export type SynthesisOutcome = Readonly<{
  candidates: readonly ParsedCandidate[];
  costUsd: Finding['cost_usd'];
  requests: number;
}>;

type SynthesizeModuleInput = Readonly<{
  deep: boolean;
  module: AuditModule;
  priorFeedback?: PriorRoundFeedback;
  root: string;
}>;

const EMPTY_FEEDBACK = {
  guards: [],
  unresolved: [],
  witnesses: [],
} as const satisfies PriorRoundFeedback;

async function readAuditPrompt(): Promise<string> {
  const promptPath = await resolveWorkerAsset('audit-prompt.md');

  return await readFile(promptPath, 'utf-8');
}

async function buildModulePayload({
  deep,
  module,
  priorFeedback = EMPTY_FEEDBACK,
  root,
}: SynthesizeModuleInput): Promise<string> {
  const packet = await readModulePacket({ module, root });
  const changedFiles = module.changedFiles ?? [];
  const lowTestFiles = module.lowTestFiles ?? [];

  return JSON.stringify({
    changedFiles,
    deep,
    docs: packet.docs,
    files: packet.files,
    lowTestFiles,
    module: module.name,
    priorGuards: priorFeedback.guards,
    priorUnresolved: priorFeedback.unresolved,
    priorWitnesses: priorFeedback.witnesses,
    relatedSources: packet.relatedSources,
    sources: packet.sources,
  });
}

/** Synthesize 2-5 testable behavioral contract candidates for one repository module. */
export async function buildModuleContracts({
  deep,
  module,
  priorFeedback = EMPTY_FEEDBACK,
  root,
}: SynthesizeModuleInput): Promise<SynthesisOutcome> {
  const instructions = await readAuditPrompt();
  const source = await buildModulePayload({ deep, module, priorFeedback, root });
  const tier = selectAuditTier({ deep });
  let costUsd: Finding['cost_usd'] = 0;
  let requests = 0;

  function onRequest(cost: Finding['cost_usd']): void {
    requests += 1;
    costUsd = cost;
  }

  const raw = await runFxJson({
    instructions,
    model: tier.model,
    onRequest,
    reasoningEffort: tier.effort,
    source,
  });

  const allowedPaths = [...module.manifests, ...module.files];

  const candidates = parseAuditCandidates({
    allowedPaths,
    manifestPaths: module.manifests,
    moduleName: module.name,
    raw,
  });

  return { candidates, costUsd, requests };
}
