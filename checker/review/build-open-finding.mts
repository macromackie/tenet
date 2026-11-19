import type { Finding } from '../types.d.mts';
import { buildFinding } from './build-finding.mts';

type BuildOpenFindingInput = {
  contract: string;
  reason: string;
};

/** Report one reason a contract still needs review. */
export function buildOpenFinding({ contract, reason }: BuildOpenFindingInput): Finding {
  return buildFinding({ check: 'discovery', contract, message: reason, outcome: 'unresolved' });
}
