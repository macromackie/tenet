import { parseBundle } from '../../schema.mts';
import type { Check, Inventory, Pin } from '../../types.d.mts';
import { hasCheckInputs } from './has-check-inputs.mts';

type UpdatePinInput = {
  readonly prior: Pin;
  readonly next: Pin;
  readonly files: Inventory['runtimeFiles'];
  readonly handEditedIds?: ReadonlySet<string>;
};

type CanKeepInput = {
  readonly check: Check;
  readonly next: Pin;
  readonly files: Inventory['runtimeFiles'];
  readonly handEditedIds?: ReadonlySet<string>;
};

type PreserveEditedInput = Readonly<{
  files: Inventory['runtimeFiles'];
  handEditedIds: ReadonlySet<string>;
  next: Pin;
  prior: Pin;
}>;

function canKeep({ check, next, files, handEditedIds }: CanKeepInput): boolean {
  if (!hasCheckInputs({ check, files })) {
    return false;
  }

  if (handEditedIds?.has(check.id) === true) {
    return true;
  }

  const assigned = next.bundle.checks.find((item) => item.id === check.id);

  if (assigned === undefined) {
    return true;
  }

  const sameEntry = assigned.subject === check.subject && assigned.testFile === check.testFile;

  return sameEntry && assigned.runtime === check.runtime && assigned.promise === check.promise;
}

function preserveHandEditedChecks({
  files,
  handEditedIds,
  next,
  prior,
}: PreserveEditedInput): Pin {
  const priorChecks = new Map<string, Check>();

  for (const item of prior.bundle.checks) {
    if (handEditedIds.has(item.id) && item.test !== undefined) {
      priorChecks.set(item.id, item);
    }
  }

  if (priorChecks.size === 0) {
    return next;
  }

  const checks = next.bundle.checks.map((item) => {
    const edited = priorChecks.get(item.id);

    if (edited === undefined || edited.runtime !== item.runtime || edited.test === undefined) {
      return item;
    }

    return {
      ...item,
      scriptFile: edited.scriptFile,
      scriptHash: edited.scriptHash,
      test: edited.test,
    };
  });

  const bundle = parseBundle({ files, value: { checks, open: next.bundle.open } });

  return { ...next, bundle };
}

function mergeSameContractPin({ prior, next, files, handEditedIds }: UpdatePinInput): Pin {
  const kept = prior.bundle.checks.filter((check) =>
    canKeep({ check, files, handEditedIds, next }),
  );

  const previous = kept.map((check) => check.id);
  const ids: ReadonlySet<string> = new Set(previous);
  const additions = next.bundle.checks.filter((check) => !ids.has(check.id));
  const value = { checks: [...kept, ...additions], open: next.bundle.open };
  const bundle = parseBundle({ files, value });

  return { ...next, bundle };
}

/** Keep surviving observations of the same requirement, target and promise. */
export function updatePin({ prior, next, files, handEditedIds }: UpdatePinInput): Pin {
  if (next.contractHash === undefined || prior.contractHash !== next.contractHash) {
    if (handEditedIds !== undefined && handEditedIds.size > 0) {
      return preserveHandEditedChecks({ files, handEditedIds, next, prior });
    }

    return next;
  }

  return mergeSameContractPin({ files, handEditedIds, next, prior });
}
