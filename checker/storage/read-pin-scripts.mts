import { dirname, join } from 'node:path';
import { CheckerError } from '../checker-error.mts';
import type { Check, Pin } from '../types.d.mts';
import { buildCheckEntry } from './build-check-entry.mts';
import { hashSource } from './hash-source.mts';
import { isSafeScriptFile } from './is-safe-script-file.mts';
import { readRegular } from './read-regular.mts';

const MAX_SCRIPT_BYTES = 262_144;

type ReadPinScriptsInput = Readonly<{ lockPath: string; pin: Pin; root: string }>;
type ReadPinScriptsResult = Readonly<{ handEditedIds: ReadonlySet<string>; pin: Pin }>;

type ReadOneCheckInput = Readonly<{ check: Check; lockDir: string; root: string }>;
type ReadOneCheckResult = Readonly<{ check: Check; handEdited: boolean }>;

async function readOneCheck({
  check,
  lockDir,
  root,
}: ReadOneCheckInput): Promise<ReadOneCheckResult> {
  if (check.scriptFile === undefined) {
    return { check, handEdited: false };
  }

  if (!isSafeScriptFile(check.scriptFile)) {
    throw new CheckerError({ message: 'the saved checker references an invalid script file.' });
  }

  const scriptPath = join(lockDir, check.scriptFile);
  const diskCode = await readRegular({ limit: MAX_SCRIPT_BYTES, path: scriptPath, root });
  const handEdited = check.scriptHash !== undefined && hashSource(diskCode) !== check.scriptHash;

  const hydrated = buildCheckEntry({
    check,
    scriptFile: check.scriptFile,
    scriptHash: check.scriptHash,
    test: diskCode,
  });

  return { check: hydrated, handEdited };
}

/** Populate each pin check's runnable script from `<dirname(lockPath)>/<scriptFile>` and detect hand edits. */
export async function readPinScripts({
  lockPath,
  pin,
  root,
}: ReadPinScriptsInput): Promise<ReadPinScriptsResult> {
  const lockDir = dirname(lockPath);
  const handEditedIds = new Set<string>();
  const checks: Check[] = [];

  for (const check of pin.bundle.checks) {
    // Hydrate scripts sequentially inside the verified root.
    // oxlint-disable-next-line no-await-in-loop
    const result = await readOneCheck({ check, lockDir, root });

    if (result.handEdited) {
      handEditedIds.add(check.id);
    }

    checks.push(result.check);
  }

  return { handEditedIds, pin: { ...pin, bundle: { checks, open: pin.bundle.open } } };
}
