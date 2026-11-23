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
}>;

export type AttemptOutcome = Readonly<{
  changedPaths: readonly string[];
  costUsd: Finding['cost_usd'];
  error?: string;
  files?: ReadonlyMap<string, string>;
  outcomes?: readonly AuditFixOutcome[];
  requests: number;
}>;

type EvaluateProposalInput = Readonly<{
  context: AttemptContext;
  proposal: ProposeFixOutcome;
}>;

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

  if (!verified.ok) {
    return {
      changedPaths: [],
      costUsd: proposal.costUsd,
      error: verified.error,
      requests: proposal.requests,
    };
  }

  return {
    changedPaths: applied.changedPaths,
    costUsd: proposal.costUsd,
    files: applied.files,
    outcomes: verified.outcomes,
    requests: proposal.requests,
  };
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
    witnesses: context.promptWitnesses,
  });

  if (proposal.error !== undefined) {
    return {
      changedPaths: [],
      costUsd: proposal.costUsd,
      error: proposal.error,
      requests: proposal.requests,
    };
  }

  return await evaluateProposal({ context, proposal });
}
