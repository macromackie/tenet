import { randomUUID } from 'node:crypto';
import { link, mkdir, open, realpath, rename, rm, type FileHandle } from 'node:fs/promises';
import { dirname, join, relative, sep } from 'node:path';
import { CheckerError } from '../checker-error.mts';
import { type LocationInput, resolveInside } from './resolve-inside.mts';
import { resolveRoot } from './resolve-root.mts';

type WriteTextInput = LocationInput & Readonly<{ first?: boolean; text: string }>;
type DirectoryInput = Readonly<{ directory: string; root: string }>;
type StoreTextInput = Readonly<{ handle: FileHandle; text: string }>;
type InstallInput = Readonly<{ first: boolean; temp: string; target: string }>;

type WriteTempInput = Readonly<{
  first: boolean;
  parent: string;
  target: string;
  text: string;
}>;

const PRIVATE_FILE = 0o600;
const PRIVATE_DIR = 0o700;

async function ensureSegment(path: string): Promise<void> {
  await mkdir(path, { mode: PRIVATE_DIR, recursive: true });

  if ((await realpath(path)) !== path) {
    throw new CheckerError({ message: 'output directory is a symbolic link.' });
  }
}

async function ensureDirectory({ directory, root }: DirectoryInput): Promise<void> {
  await ensureSegment(root);

  const nested = relative(root, directory);

  if (nested.length === 0) {
    return;
  }

  let current = root;

  for (const part of nested.split(sep)) {
    current = join(current, part);

    // Verify each existing directory segment before creating any missing child.
    // oxlint-disable-next-line no-await-in-loop
    await ensureSegment(current);
  }
}

async function installRecord({ first, target, temp }: InstallInput): Promise<void> {
  if (first) {
    await link(temp, target);

    return;
  }

  await rename(temp, target);
}

async function storeText({ handle, text }: StoreTextInput): Promise<void> {
  try {
    await handle.writeFile(text, 'utf-8');
  } finally {
    await handle.close();
  }
}

async function writeTempText({ first, parent, target, text }: WriteTempInput): Promise<void> {
  const nonce = randomUUID();
  const temp = join(parent, `.tenet-${nonce}.tmp`);
  const handle = await open(temp, 'wx', PRIVATE_FILE);

  try {
    await storeText({ handle, text });
    await installRecord({ first, target, temp });
  } finally {
    await rm(temp, { force: true });
  }
}

/** Atomically install plain UTF-8 text inside a verified non-symlink directory tree. */
export async function writeTextAtomic(input: WriteTextInput): Promise<void> {
  const root = await resolveRoot(input.root);
  const target = resolveInside({ path: input.path, root });
  const parent = dirname(target);

  await ensureDirectory({ directory: parent, root });
  await writeTempText({ first: input.first === true, parent, target, text: input.text });
}
