import { open, realpath, rm } from 'node:fs/promises';
import { dirname } from 'node:path';
import { Option } from 'effect';
import { CheckerError } from '../checker-error.mts';
import { readRecord, type RecordData } from './read-record.mts';
import { resolveInside } from './resolve-inside.mts';
import { resolveRoot } from './resolve-root.mts';
import { writeAtomic, type WriteInput } from './write-atomic.mts';

type ReplaceInput = WriteInput & Readonly<{ prior: Option.Option<RecordData> }>;
const PRIVATE_FILE = 0o600;

async function resolvePinLock(input: ReplaceInput): Promise<string> {
  const root = await resolveRoot(input.root);
  const lock = resolveInside({ path: `${input.path}.lock`, root });
  const parent = dirname(lock);

  if ((await realpath(parent)) !== parent) {
    throw new CheckerError({ message: 'output directory is a symbolic link.' });
  }

  return lock;
}

/** Serialize replacements and publish only when the pin still matches the reviewed version. */
export async function replacePin(input: ReplaceInput): Promise<void> {
  const lock = await resolvePinLock(input);
  const claim = await open(lock, 'wx', PRIVATE_FILE);

  try {
    const actual = await readRecord(input);

    if (
      Option.isNone(actual) ||
      Option.isNone(input.prior) ||
      actual.value.text !== input.prior.value.text
    ) {
      throw new CheckerError({ message: 'the pin changed during qualification. Rerun the check.' });
    }

    await writeAtomic(input);
  } finally {
    await claim.close();
    await rm(lock, { force: true });
  }
}
