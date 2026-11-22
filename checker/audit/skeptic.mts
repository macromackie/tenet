import { Result } from 'effect';
import { classifyResult } from '../classify-result.mts';
import { runFxJson } from '../fx/index.mts';
import type { Finding } from '../types.d.mts';
import { applySkepticOutput } from './apply-skeptic.mts';
import { selectAuditTier } from './model-tier.mts';
import { readModulePacket } from './packet.mts';
import type { AuditModule } from './wire.mts';

const SKEPTIC_INSTRUCTIONS = [
  'You are a strict software verification skeptic triaging live failing behavioral witnesses against module source code, docs, and existing repository tests (`tests`).',
  'Return ONLY JSON: {"reviews":[{"check":"...","verdict":"critical"|"high"|"medium"|"reject","reason":"one concise sentence"}]}.',
  'Use "reject" when an existing test in `tests` asserts or exercises the observed behavior as intended, when the witness contradicts documented design or preconditions, relies on an unexported internal quirk, passes unsupported input, or nitpicks style rather than a concrete defect.',
  'Use "critical" for unhandled crashes/panics/exceptions on valid input, data loss, security boundary violations, or unexpected mutation of caller-owned inputs or receivers.',
  'Use "high" for concrete wrong results, round-trip or metamorphic contradictions, cross-API inconsistencies, or clear violations of documented guarantees.',
  'Use "medium" for genuine but narrow edge-case defects with limited impact.',
  'Be skeptical: prefer "reject" over inflating an intentional or test-blessed quirk into a defect.',
].join('\n');

type SkepticOutcome = Readonly<{
  costUsd: Finding['cost_usd'];
  findings: readonly Finding[];
  requests: number;
}>;

type ReviewWitnessesInput = Readonly<{
  deep: boolean;
  findings: readonly Finding[];
  module: AuditModule;
  root: string;
}>;

type BuildPayloadInput = Readonly<{
  failed: readonly Finding[];
  module: AuditModule;
  root: string;
}>;

type SkepticCallInput = Readonly<{
  deep: boolean;
  source: string;
}>;

async function buildSkepticPayload({ failed, module, root }: BuildPayloadInput): Promise<string> {
  const packet = await readModulePacket({ includeTests: true, module, root });

  const witnesses = failed.map((item) => ({
    check: item.check,
    contract: item.contract,
    message: item.message,
    subject: item.subject,
  }));

  return JSON.stringify({
    docs: packet.docs,
    module: module.name,
    sources: packet.sources,
    tests: packet.tests,
    witnesses,
  });
}

async function callSkepticModel({ deep, source }: SkepticCallInput) {
  const tier = selectAuditTier({ deep });
  let costUsd: Finding['cost_usd'] = 0;
  let requests = 0;

  function onRequest(cost: Finding['cost_usd']): void {
    requests += 1;
    costUsd = cost;
  }

  async function runReview(): Promise<unknown> {
    return await runFxJson({
      instructions: SKEPTIC_INSTRUCTIONS,
      model: tier.model,
      onRequest,
      reasoningEffort: tier.effort,
      source,
    });
  }

  const outcome = await classifyResult({
    message: 'skeptic witness review failed',
    run: runReview,
  });

  return { costUsd, outcome, requests };
}

/** Run one batched skeptic review over a module's live failing witnesses to filter false positives. */
export async function reviewFailingWitnesses({
  deep,
  findings,
  module,
  root,
}: ReviewWitnessesInput): Promise<SkepticOutcome> {
  const failed = findings.filter((row) => row.outcome === 'failed');

  if (failed.length === 0) {
    return { costUsd: 0, findings, requests: 0 };
  }

  const source = await buildSkepticPayload({ failed, module, root });
  const { costUsd, outcome, requests } = await callSkepticModel({ deep, source });

  if (Result.isFailure(outcome)) {
    return { costUsd, findings, requests };
  }

  const reviewed = applySkepticOutput({ findings, raw: outcome.success });

  return { costUsd, findings: reviewed, requests };
}
