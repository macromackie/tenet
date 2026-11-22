import { constants, type Stats } from 'node:fs';
import { lstat, open, realpath, type FileHandle } from 'node:fs/promises';
import { CheckerError } from '../checker-error.mts';
import { type LocationInput, resolveInside } from './resolve-inside.mts';
import { resolveRoot } from './resolve-root.mts';

type ReadInput = LocationInput &
  Readonly<{ limit: number; preserveBom?: boolean; truncate?: boolean }>;

type DecodeInput = Readonly<{
  buffer: Buffer;
  limit: number;
  preserveBom?: boolean;
  totalBytes: number;
  truncate: boolean;
  used: number;
}>;

type TextInput = Readonly<{
  handle: FileHandle;
  limit: number;
  preserveBom?: boolean;
  totalBytes: number;
  truncate: boolean;
}>;

type OpenInput = TextInput & Readonly<{ file: string; prior: Stats }>;

type TrimPrefixInput = Readonly<{ text: string; totalBytes: number }>;

/** Combine Node open flags to avoid following links or blocking on a FIFO. */
// oxlint-disable-next-line no-bitwise
const OPEN_FLAGS = constants.O_RDONLY | constants.O_NONBLOCK | constants.O_NOFOLLOW;

type CapacityInput = Readonly<{ limit: number; truncate: boolean }>;

type UnavailableInput = Readonly<{
  file: string;
  limit: number;
  prior: Stats;
  realPath: string;
  truncate: boolean;
}>;

type CheckedFileInput = LocationInput & Readonly<{ limit: number; truncate: boolean }>;

function readBufferCapacity({ limit, truncate }: CapacityInput): number {
  if (truncate) {
    return limit;
  }

  return limit + 1;
}

function sliceBeforeLastNewline(text: string): string {
  const lastNewline = text.lastIndexOf('\n');

  if (lastNewline > 0) {
    return text.slice(0, lastNewline);
  }

  return text;
}

function trimTruncatedPrefix({ text, totalBytes }: TrimPrefixInput): string {
  const sliced = sliceBeforeLastNewline(text);
  const shownBytes = Buffer.byteLength(sliced);
  const shown = String(shownBytes);
  const total = String(totalBytes);

  return `${sliced}\n[truncated: showing first ${shown} of ${total} bytes]\n`;
}

function isUnavailableFile({
  file,
  limit,
  prior,
  realPath,
  truncate,
}: UnavailableInput): boolean {
  if (!prior.isFile() || realPath !== file) {
    return true;
  }

  return !truncate && prior.size > limit;
}

function decodeText({
  buffer,
  limit,
  preserveBom = false,
  totalBytes,
  truncate,
  used,
}: DecodeInput): string {
  const data = buffer.subarray(0, used);

  if ((!truncate && used > limit) || data.includes(0)) {
    throw new CheckerError({ message: 'incomplete text input.' });
  }

  // ast-grep-ignore: acronym-names-typescript -- The WHATWG TextDecoderOptions dictionary spells this member ignoreBOM.
  const decoder = new TextDecoder('utf-8', { fatal: !truncate, ignoreBOM: preserveBom });
  const text = decoder.decode(data);

  if (truncate && totalBytes > used) {
    return trimTruncatedPrefix({ text, totalBytes });
  }

  return text;
}

async function readText({
  handle,
  limit,
  preserveBom,
  totalBytes,
  truncate,
}: TextInput): Promise<string> {
  const capacity = readBufferCapacity({ limit, truncate });
  const buffer = Buffer.alloc(capacity);
  let used = 0;

  while (used < buffer.length) {
    // The next bounded read uses the byte count from the preceding read.
    // oxlint-disable-next-line no-await-in-loop
    const result = await handle.read(buffer, used, buffer.length - used);

    if (result.bytesRead === 0) {
      break;
    }

    used += result.bytesRead;
  }

  return decodeText({ buffer, limit, preserveBom, totalBytes, truncate, used });
}

async function readOpened(input: OpenInput): Promise<string> {
  const { handle, prior, file } = input;
  const stat = await handle.stat();

  if (stat.dev !== prior.dev || stat.ino !== prior.ino || (await realpath(file)) !== file) {
    throw new CheckerError({ message: 'input changed while opening it.' });
  }

  return await readText(input);
}

async function resolveCheckedFile({
  limit,
  path,
  root: rawRoot,
  truncate,
}: CheckedFileInput): Promise<Readonly<{ file: string; prior: Stats }>> {
  const root = await resolveRoot(rawRoot);
  const file = resolveInside({ path, root });
  const prior = await lstat(file);
  const realPath = await realpath(file);

  if (isUnavailableFile({ file, limit, prior, realPath, truncate })) {
    throw new CheckerError({ message: 'input unavailable.' });
  }

  return { file, prior };
}

/** Read a limited UTF-8 regular file and reject changed files, FIFOs, symlinks and binary content. */
export async function readRegular({
  limit,
  path,
  preserveBom,
  root,
  truncate = false,
}: ReadInput): Promise<string> {
  const { file, prior } = await resolveCheckedFile({ limit, path, root, truncate });
  const handle = await open(file, OPEN_FLAGS);

  try {
    return await readOpened({
      file,
      handle,
      limit,
      preserveBom,
      prior,
      totalBytes: prior.size,
      truncate,
    });
  } finally {
    await handle.close();
  }
}
