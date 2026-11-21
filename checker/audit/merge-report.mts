import type { Finding } from '../types.d.mts';
import type { AuditContractOutput, AuditModuleReport } from './wire.mts';

type SumCostInput = Readonly<{
  first: Finding['cost_usd'];
  second: Finding['cost_usd'];
}>;

type MergeContractsInput = Readonly<{
  first: readonly AuditContractOutput[];
  second: readonly AuditContractOutput[];
}>;

type MergeReportsInput = Readonly<{
  base: AuditModuleReport;
  extra: AuditModuleReport;
}>;

function addCost({ first, second }: SumCostInput): Finding['cost_usd'] {
  if (first === null || second === null) {
    return null;
  }

  return first + second;
}

function mergeContracts({ first, second }: MergeContractsInput): readonly AuditContractOutput[] {
  const names = first.map((item) => item.name);
  const seenNames = new Set(names);
  const merged = [...first];

  for (const item of second) {
    if (!seenNames.has(item.name)) {
      seenNames.add(item.name);
      merged.push(item);
    }
  }

  return merged;
}

/** Merge two audit rounds for the same module, deduplicating contracts by name. */
export function mergeModuleReports({ base, extra }: MergeReportsInput): AuditModuleReport {
  return {
    contracts: mergeContracts({ first: base.contracts, second: extra.contracts }),
    costUsd: addCost({ first: base.costUsd, second: extra.costUsd }),
    findings: [...base.findings, ...extra.findings],
    name: base.name,
    requests: base.requests + extra.requests,
  };
}
