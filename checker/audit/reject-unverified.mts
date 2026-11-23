import type { Finding } from '../types.d.mts';

/** Downgrade unverified brownfield witnesses to unresolved when skeptic review cannot confirm them. */
export function rejectUnverifiedWitnesses(findings: readonly Finding[]): readonly Finding[] {
  return findings.map((row) => {
    if (row.outcome === 'failed' && row.brownfield_unverified === true) {
      return {
        ...row,
        message: `unverified brownfield witness downgraded: skeptic review unavailable (${row.message})`,
        outcome: 'unresolved',
      };
    }

    return row;
  });
}
