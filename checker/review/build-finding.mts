import type { Finding } from '../types.d.mts';

const SUMMARY_LIMIT = 512;

type BuildFindingInput = Pick<Finding, 'check' | 'contract' | 'message' | 'outcome'> &
  Partial<Pick<Finding, 'pin_reused' | 'repro' | 'reused' | 'subject'>>;

/** Build a single line of terminal-safe evidence. */
export function buildFinding({
  pin_reused,
  repro,
  subject = null,
  ...note
}: BuildFindingInput): Finding {
  const message = note.message
    // Strip terminal controls from untrusted observations.
    // oxlint-disable-next-line no-control-regex
    .replaceAll(/[\u0000-\u001F\u007F-\u009F\u2028\u2029\u202A-\u202E\u2066-\u2069]/gv, ' ')
    .slice(0, SUMMARY_LIMIT);

  const finding: Finding = {
    ...note,
    cost_usd: null,
    message,
    requested: false,
    reused: note.reused === true,
    subject,
  };

  if (pin_reused === true && repro !== undefined) {
    return { ...finding, pin_reused: true, repro };
  }

  if (pin_reused === true) {
    return { ...finding, pin_reused: true };
  }

  if (repro !== undefined) {
    return { ...finding, repro };
  }

  return finding;
}
