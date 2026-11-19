import type { Contract, Finding } from '../types.d.mts';
import { buildFinding } from './build-finding.mts';

type ReportCoverageInput = { readonly contract: Contract; readonly omitted: readonly string[] };

/** Expose file-level gaps without hiding observations on covered targets. */
export function reportCoverage({ omitted, contract }: ReportCoverageInput): readonly Finding[] {
  if (omitted.length === 0) {
    return [];
  }

  const count = String(omitted.length);

  return [
    buildFinding({
      check: 'coverage',
      contract: contract.name,
      message: `${count} source files could not be covered. First: ${omitted[0]}`,
      outcome: 'unresolved',
    }),
  ];
}
