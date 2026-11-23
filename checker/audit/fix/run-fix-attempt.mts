import {
  type FixPromptWitness,
  type ProposeFixOutcome,
  requestFixProposal,
} from '../../fx/index.mts';
import type { Finding } from '../../types.d.mts';
import { selectAuditTier } from '../model-tier.mts';
import { applyAuditFix } from './apply-fix-edits.mts';
import type { AuditFixGroup, AuditFixOutcome } from './fix-wire.mts';
import { type PendingWitness, verifyFixPatch } from './verify-fix-patch.mts';

export type AttemptContext = Readonly<{
  currentFiles: ReadonlyMap<string, string>;
  deep: boolean;
  group: AuditFixGroup;
  pending: readonly PendingWitness[];
  priorAttemptError?: string;
  promptWitnesses: readonly FixPromptWitness[];
  relatedSources: Readonly<Record<string, string>>;
  root: string;
  sources: Readonly<Record<string, string>>;
  state: string;
  tests: Readonly<Record<string, string>>;
}>;

export type AttemptOutcome = Readonly<{
  changedPaths: readonly string[];
  costUsd: Finding['cost_usd'];
  error?: string;
  files?: ReadonlyMap<string, string>;
  fixedCount: number;
  guardRegressions: number;
  outcomes?: readonly AuditFixOutcome[];
  requests: number;
}>;

type EvaluateProposalInput = Readonly<{
  context: AttemptContext;
  proposal: ProposeFixOutcome;
}>;

type BuildVerifiedOutcomeInput = Readonly<{
  changedPaths: readonly string[];
  costUsd: Finding['cost_usd'];
  files: ReadonlyMap<string, string>;
  requests: number;
  verified: Awaited<ReturnType<typeof verifyFixPatch>>;
}>;

function buildVerifiedOutcome({
  changedPaths,
  costUsd,
  files,
  requests,
  verified,
}: BuildVerifiedOutcomeInput): AttemptOutcome {
  if (verified.ok) {
    return {
      changedPaths,
      costUsd,
      files,
      fixedCount: verified.fixedCount,
      guardRegressions: 0,
      outcomes: verified.outcomes,
      requests,
    };
  }

  if (verified.fixedCount > 0 && verified.guardRegressions === 0) {
    return {
      changedPaths,
      costUsd,
      error: verified.error,
      files,
      fixedCount: verified.fixedCount,
      guardRegressions: 0,
      outcomes: verified.outcomes,
      requests,
    };
  }

  return {
    changedPaths: [],
    costUsd,
    error: verified.error,
    fixedCount: verified.fixedCount,
    guardRegressions: verified.guardRegressions,
    requests,
  };
}

async function evaluateProposal({
  context,
  proposal,
}: EvaluateProposalInput): Promise<AttemptOutcome> {
  const applied = applyAuditFix({
    allowedFiles: context.group.files,
    files: context.currentFiles,
    raw: proposal.raw,
  });

  if (!applied.ok) {
    return {
      changedPaths: [],
      costUsd: proposal.costUsd,
      error: applied.error,
      fixedCount: 0,
      guardRegressions: 0,
      requests: proposal.requests,
    };
  }

  const verified = await verifyFixPatch({
    files: applied.files,
    group: context.group,
    pending: context.pending,
    root: context.root,
    state: context.state,
  });

  return buildVerifiedOutcome({
    changedPaths: applied.changedPaths,
    costUsd: proposal.costUsd,
    files: applied.files,
    requests: proposal.requests,
    verified,
  });
}

/** Propose one candidate fix via fx/, apply edits in memory, and verify witnesses and guards. */
export async function runFixAttempt(context: AttemptContext): Promise<AttemptOutcome> {
  const tier = selectAuditTier({ deep: context.deep });

  const proposal = await requestFixProposal({
    contracts: context.group.contracts,
    files: context.group.files,
    guards: context.group.guards,
    model: tier.model,
    moduleName: context.group.name,
    priorAttemptError: context.priorAttemptError,
    reasoningEffort: tier.effort,
    relatedSources: context.relatedSources,
    sources: context.sources,
    tests: context.tests,
    witnesses: context.promptWitnesses,
  });

  if (proposal.error !== undefined) {
    return {
      changedPaths: [],
      costUsd: proposal.costUsd,
      error: proposal.error,
      fixedCount: 0,
      guardRegressions: 0,
      requests: proposal.requests,
    };
  }

  return await evaluateProposal({ context, proposal });
}
