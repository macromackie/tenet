import { parseBundle } from '../schema.mts';
import type { Check, Inventory, Pin } from '../types.d.mts';
import { hasCheckInputs } from './has-check-inputs.mts';

type UpdatePinInput = {
  readonly prior: Pin;
  readonly next: Pin;
  readonly files: Inventory['runtimeFiles'];
};

type CanKeepInput = {
  readonly check: Check;
  readonly next: Pin;
  readonly files: Inventory['runtimeFiles'];
};

function canKeep({ check, next, files }: CanKeepInput): boolean {
  if (!hasCheckInputs({ check, files })) {
    return false;
  }

  const assigned = next.bundle.checks.find((item) => item.id === check.id);

  if (assigned === undefined) {
    return true;
  }

  const sameEntry = assigned.subject === check.subject && assigned.testFile === check.testFile;

  return sameEntry && assigned.runtime === check.runtime && assigned.promise === check.promise;
}

/** Keep surviving observations of the same requirement, target and promise. */
export function updatePin({ prior, next, files }: UpdatePinInput): Pin {
  if (next.contractHash === undefined || prior.contractHash !== next.contractHash) {
    return next;
  }

  const kept = prior.bundle.checks.filter((check) => canKeep({ check, files, next }));
  const previous = kept.map((check) => check.id);
  const ids: ReadonlySet<string> = new Set(previous);
  const additions = next.bundle.checks.filter((check) => !ids.has(check.id));
  const value = { checks: [...kept, ...additions], open: next.bundle.open };
  const bundle = parseBundle({ files, value });

  return { ...next, bundle };
}
