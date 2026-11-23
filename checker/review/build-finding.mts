import type { Finding } from '../types.d.mts';

const SUMMARY_LIMIT = 512;

type BuildFindingInput = Pick<Finding, 'check' | 'contract' | 'message' | 'outcome'> &
  Partial<Pick<Finding, 'brownfield_unverified' | 'pin_reused' | 'repro' | 'reused' | 'subject'>>;

type OptionalFlagsInput = Readonly<{
  brownfield_unverified?: boolean;
  finding: Finding;
  pin_reused?: boolean;
  repro?: Finding['repro'];
}>;

type WithPinInput = Readonly<{
  finding: Finding;
  pin_reused?: boolean;
}>;

function attachPinFlag({ finding, pin_reused }: WithPinInput): Finding {
  if (pin_reused === true) {
    return { ...finding, pin_reused: true };
  }

  return finding;
}

function attachOptionalMetadata({
  brownfield_unverified,
  finding,
  pin_reused,
  repro,
}: OptionalFlagsInput): Finding {
  const base = attachPinFlag({ finding, pin_reused });

  if (repro !== undefined && brownfield_unverified === true) {
    return { ...base, brownfield_unverified: true, repro };
  }

  if (repro !== undefined) {
    return { ...base, repro };
  }

  if (brownfield_unverified === true) {
    return { ...base, brownfield_unverified: true };
  }

  return base;
}

/** Build a single line of terminal-safe evidence. */
export function buildFinding({
  brownfield_unverified,
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

  return attachOptionalMetadata({ brownfield_unverified, finding, pin_reused, repro });
}
