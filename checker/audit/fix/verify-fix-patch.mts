import { replayTarget } from '../replay-target.mts';
import type { AuditReplayResult, ReplayTarget } from '../replay-wire.mts';
import { buildFixOutcome } from './build-fix-outcome.mts';
import type {
  AuditFixGroup,
  AuditFixOutcome,
  AuditFixOutcomeStatus,
  AuditFixWitness,
} from './fix-wire.mts';

export type PendingWitness = Readonly<{
  initial: AuditReplayResult;
  witness: AuditFixWitness;
}>;

export type PrecheckResult = Readonly<{
  alreadyPassing: readonly AuditFixOutcome[];
  pending: readonly PendingWitness[];
  precheckUnresolved: readonly AuditFixOutcome[];
}>;

type PatchVerification =
  | Readonly<{
      fixedCount: number;
      guardRegressions: number;
      ok: true;
      outcomes: readonly AuditFixOutcome[];
    }>
  | Readonly<{
      error: string;
      fixedCount: number;
      guardRegressions: number;
      ok: false;
      outcomes: readonly AuditFixOutcome[];
    }>;

type ReplayBatchInput = Readonly<{
  files: ReadonlyMap<string, string>;
  root: string;
  state: string;
  targets: readonly ReplayTarget[];
}>;

type DiagnosticsInput = Readonly<{
  guardResults: readonly AuditReplayResult[];
  witnessResults: readonly AuditReplayResult[];
}>;

type VerifyPatchInput = Readonly<{
  files: ReadonlyMap<string, string>;
  group: AuditFixGroup;
  pending: readonly PendingWitness[];
  root: string;
  state: string;
}>;

async function replayTargets({
  files,
  root,
  state,
  targets,
}: ReplayBatchInput): Promise<readonly AuditReplayResult[]> {
  const results: AuditReplayResult[] = [];

  for (const target of targets) {
    // Sequential replay keeps sandbox execution bounded per attempt.
    // oxlint-disable-next-line no-await-in-loop
    const result = await replayTarget({ files, root, state, target });

    results.push(result);
  }

  return results;
}

function collectDiagnosticLines({
  guardResults,
  witnessResults,
}: DiagnosticsInput): readonly string[] {
  const lines: string[] = [];

  for (const item of witnessResults) {
    if (item.outcome !== 'passed') {
      lines.push(`witness ${item.contract}/${item.check} still ${item.outcome}: ${item.message}`);
    }
  }

  for (const item of guardResults) {
    if (item.outcome !== 'passed') {
      lines.push(
        `guard ${item.contract}/${item.check} regressed (${item.outcome}): ${item.message}`,
      );
    }
  }

  return lines;
}

function mapWitnessOutcomeStatus(result: AuditReplayResult): AuditFixOutcomeStatus {
  if (result.outcome === 'passed') {
    return 'fixed';
  }

  if (result.outcome === 'failed') {
    return 'failed';
  }

  return 'unresolved';
}

function buildWitnessOutcomes(
  witnessResults: readonly AuditReplayResult[],
): readonly AuditFixOutcome[] {
  return witnessResults.map((result) => {
    const status = mapWitnessOutcomeStatus(result);

    return buildFixOutcome({ result, status });
  });
}

/** Replay all pending witnesses and guards against a patched in-memory file map inside the sandbox. */
export async function verifyFixPatch({
  files,
  group,
  pending,
  root,
  state,
}: VerifyPatchInput): Promise<PatchVerification> {
  const witnessTargets = pending.map((item) => item.witness);
  const witnessResults = await replayTargets({ files, root, state, targets: witnessTargets });
  const guardResults = await replayTargets({ files, root, state, targets: group.guards });
  const diagnostics = collectDiagnosticLines({ guardResults, witnessResults });
  const fixedCount = witnessResults.filter((item) => item.outcome === 'passed').length;
  const guardRegressions = guardResults.filter((item) => item.outcome !== 'passed').length;
  const outcomes = buildWitnessOutcomes(witnessResults);

  if (diagnostics.length > 0) {
    return {
      error: diagnostics.join('\n'),
      fixedCount,
      guardRegressions,
      ok: false,
      outcomes,
    };
  }

  return { fixedCount, guardRegressions: 0, ok: true, outcomes };
}
