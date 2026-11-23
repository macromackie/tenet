import { readFile } from 'node:fs/promises';
import { Result } from 'effect';
import { classifyResult } from '../classify-result.mts';
import type { Finding } from '../types.d.mts';
import { runFxJson } from './fx-json.mts';
import type { ReasoningEffort } from './openrouter.mts';
import { resolveWorkerAsset } from './resolve-asset.mts';

type FixPromptContract = Readonly<{
  document: string;
  lockPath: string;
  message: string;
  name: string;
  paths: readonly string[];
}>;

type FixPromptGuard = Readonly<{
  check: string;
  contract: string;
  lockPath?: string;
  subject?: string;
}>;

type FixPromptRepro = Readonly<{
  code: string;
  expected: string;
  observed?: string;
  runtime: string;
}>;

export type FixPromptWitness = Readonly<{
  check: string;
  contract: string;
  expected?: string;
  message: string;
  observed?: string;
  repro?: FixPromptRepro;
  subject?: string;
}>;

type ProposeFixInput = Readonly<{
  contracts: readonly FixPromptContract[];
  files: readonly string[];
  guards: readonly FixPromptGuard[];
  model?: string;
  moduleName: string;
  priorAttemptError?: string;
  reasoningEffort?: ReasoningEffort;
  relatedSources: Readonly<Record<string, string>>;
  sources: Readonly<Record<string, string>>;
  tests: Readonly<Record<string, string>>;
  witnesses: readonly FixPromptWitness[];
}>;

export type ProposeFixOutcome = Readonly<{
  costUsd: Finding['cost_usd'];
  error?: string;
  raw?: unknown;
  requests: number;
}>;

async function readFixPrompt(): Promise<string> {
  const promptPath = await resolveWorkerAsset('fix-prompt.md');

  return await readFile(promptPath, 'utf-8');
}

function buildFixPayload({
  contracts,
  files,
  guards,
  moduleName,
  priorAttemptError,
  relatedSources,
  sources,
  tests,
  witnesses,
}: ProposeFixInput): string {
  const base = {
    contracts,
    files,
    guards,
    module: moduleName,
    relatedSources,
    sources,
    tests,
    witnesses,
  };

  if (priorAttemptError !== undefined && priorAttemptError.length > 0) {
    return JSON.stringify({ ...base, priorAttemptError });
  }

  return JSON.stringify(base);
}

function readProposalFailure(cause: unknown): string {
  if (cause instanceof Error && cause.message.length > 0) {
    return cause.message;
  }

  return 'could not generate a candidate fix';
}

type FormatOutcomeInput = Readonly<{
  costUsd: Finding['cost_usd'];
  outcome: Result.Result<unknown, { readonly cause?: unknown }>;
  requests: number;
}>;

function formatProposalOutcome({
  costUsd,
  outcome,
  requests,
}: FormatOutcomeInput): ProposeFixOutcome {
  if (Result.isFailure(outcome)) {
    const error = readProposalFailure(outcome.failure.cause);

    return { costUsd, error, requests };
  }

  return { costUsd, raw: outcome.success, requests };
}

/** Request one minimal JSON repair proposal for a module's failing witnesses via fx/. */
export async function requestFixProposal(input: ProposeFixInput): Promise<ProposeFixOutcome> {
  const instructions = await readFixPrompt();
  const source = buildFixPayload(input);
  let costUsd: Finding['cost_usd'] = 0;
  let requests = 0;

  function onRequest(cost: Finding['cost_usd']): void {
    requests += 1;
    costUsd = cost;
  }

  async function runTurn(): Promise<unknown> {
    return await runFxJson({
      instructions,
      model: input.model,
      onRequest,
      reasoningEffort: input.reasoningEffort,
      source,
    });
  }

  const outcome = await classifyResult({ message: 'fix proposal failed', run: runTurn });

  return formatProposalOutcome({ costUsd, outcome, requests });
}
