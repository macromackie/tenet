import type { AuditReplayResult } from '../replay-wire.mts';
import type { AuditFixOutcome, AuditFixOutcomeStatus } from './fix-wire.mts';

type BuildFixOutcomeInput = Readonly<{
  result: AuditReplayResult;
  status: AuditFixOutcomeStatus;
}>;

/** Convert a sandbox replay result into an audit-fix wire outcome entry. */
export function buildFixOutcome({ result, status }: BuildFixOutcomeInput): AuditFixOutcome {
  return {
    check: result.check,
    contract: result.contract,
    expected: result.expected,
    message: result.message,
    observed: result.observed,
    status,
    subject: result.subject,
  };
}
