import type { Check, Finding } from '../types.d.mts';
import type { CandidateEvaluation } from './pin/index.mts';

type MergeEvaluationsInput = Readonly<{
  prior: CandidateEvaluation;
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

type CollectUnresolvedInput = Readonly<{
  qualifiedIds: ReadonlySet<string>;
  repaired: CandidateEvaluation;
}>;

function collectUnresolved({ qualifiedIds, repaired }: CollectUnresolvedInput): readonly Finding[] {
  return repaired.extraUnresolved.filter((finding) => !qualifiedIds.has(finding.check));
}

/** Combine previously qualified checks with newly repaired candidates without losing progress. */
export function mergeEvaluations({ prior, repaired }: MergeEvaluationsInput): CandidateEvaluation {
  const qualified = collectQualified({ prior, repaired });
  const qualifiedIdList = qualified.map((check) => check.id);
  const qualifiedIds = new Set<string>(qualifiedIdList);
  const extraUnresolved = collectUnresolved({ qualifiedIds, repaired });
  const open = collectOpen({ prior, repaired });

  return {
    errors: repaired.errors,
    extraUnresolved,
    open,
    qualified,
  };
}
