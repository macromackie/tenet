import { isMissingExternalDepMessage } from '../sandbox/index.mts';
import type { Check, Finding } from '../types.d.mts';
import type { CandidateEvaluation } from './pin/index.mts';

type MergeEvaluationsInput = Readonly<{
  prior: CandidateEvaluation;
  repaired: CandidateEvaluation;
}>;

type CollectUnresolvedInput = Readonly<{
  prior: CandidateEvaluation;
  qualifiedIds: ReadonlySet<string>;
  repaired: CandidateEvaluation;
}>;

function collectQualified({ prior, repaired }: MergeEvaluationsInput): readonly Check[] {
  const seenIds = new Set<string>();
  const merged: Check[] = [];

  for (const check of [...prior.qualified, ...repaired.qualified]) {
    if (!seenIds.has(check.id)) {
      seenIds.add(check.id);
      merged.push(check);
    }
  }

  return merged;
}

function collectOpen({ prior, repaired }: MergeEvaluationsInput): readonly string[] {
  const openItems = new Set<string>([...prior.open, ...repaired.open]);

  return [...openItems];
}

function collectUnrecoverableErrors({ prior, repaired }: MergeEvaluationsInput): readonly string[] {
  const unrecoverableItems = new Set<string>([
    ...(prior.unrecoverableErrors ?? []),
    ...(repaired.unrecoverableErrors ?? []),
  ]);

  return [...unrecoverableItems];
}

function isPreservedPriorFinding(finding: Finding): boolean {
  return finding.brownfield_unverified === true || isMissingExternalDepMessage(finding.message);
}

function collectUnresolved({
  prior,
  qualifiedIds,
  repaired,
}: CollectUnresolvedInput): readonly Finding[] {
  const activeRepaired = repaired.extraUnresolved.filter(
    (finding) => !qualifiedIds.has(finding.check),
  );

  const repairedIdList = activeRepaired.map((finding) => finding.check);
  const coveredIds = new Set<string>([...qualifiedIds, ...repairedIdList]);

  const preservedPrior = prior.extraUnresolved.filter(
    (finding) => !coveredIds.has(finding.check) && isPreservedPriorFinding(finding),
  );

  return [...preservedPrior, ...activeRepaired];
}

/** Combine previously qualified checks with newly repaired candidates without losing progress. */
export function mergeEvaluations({ prior, repaired }: MergeEvaluationsInput): CandidateEvaluation {
  const qualified = collectQualified({ prior, repaired });
  const qualifiedIdList = qualified.map((check) => check.id);
  const qualifiedIds = new Set<string>(qualifiedIdList);
  const extraUnresolved = collectUnresolved({ prior, qualifiedIds, repaired });
  const open = collectOpen({ prior, repaired });
  const unrecoverableErrors = collectUnrecoverableErrors({ prior, repaired });

  return {
    errors: repaired.errors,
    extraUnresolved,
    open,
    qualified,
    unrecoverableErrors,
  };
}
