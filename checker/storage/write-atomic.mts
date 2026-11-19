import { randomUUID } from 'node:crypto';
import { link, mkdir, open, realpath, rename, rm, type FileHandle } from 'node:fs/promises';
import { dirname, join } from 'node:path';
import { CheckerError } from '../checker-error.mts';
import { type LocationInput, resolveInside } from './resolve-inside.mts';

export type WriteInput = LocationInput & Readonly<{ first?: boolean; value: unknown }>;
type StoreInput = Readonly<{ handle: FileHandle; record: unknown }>;
type InstallInput = Readonly<{ first: boolean; temp: string; target: string }>;
const PRIVATE_FILE = 0o600;
const PRIVATE_DIR = 0o700;

async function ensureDirectory(path: string): Promise<void> {
  const parent = dirname(path);

  if (parent !== path) {
    await ensureDirectory(parent);
    // Verify each existing ancestor before creating any missing child.
    await mkdir(path, { mode: PRIVATE_DIR, recursive: true });
  }

  if ((await realpath(path)) !== path) {
    throw new CheckerError({ message: 'output directory is a symbolic link.' });
  }
}

async function installRecord({ first, target, temp }: InstallInput): Promise<void> {
  if (first) {
    await link(temp, target);

    return;
  }

  await rename(temp, target);
}

async function storeRecord({ handle, record }: StoreInput): Promise<void> {
  try {
    const content = JSON.stringify(record);

    await handle.writeFile(`${content}\n`);
  } finally {
    await handle.close();
  }
}

/** Install an immutable first pin or replace an observation as a single directory operation. */
export async function writeAtomic(input: WriteInput): Promise<void> {
  const target = resolveInside(input);
  const parent = dirname(target);

  await ensureDirectory(parent);

  const nonce = randomUUID();
  const temp = join(parent, `.tenet-${nonce}.tmp`);

  const handle = await open(temp, 'wx', PRIVATE_FILE);

  try {
    await storeRecord({ handle, record: input.value });
    await installRecord({ first: input.first === true, target, temp });
  } finally {
    await rm(temp, { force: true });
  }
}
