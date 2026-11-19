import { constants, type Stats } from 'node:fs';
import { lstat, open, realpath, type FileHandle } from 'node:fs/promises';
import { CheckerError } from '../checker-error.mts';
import { type LocationInput, resolveInside } from './resolve-inside.mts';

type ReadInput = LocationInput & Readonly<{ limit: number; preserveBom?: boolean }>;
type DecodeInput = Readonly<{ buffer: Buffer; used: number; limit: number; preserveBom?: boolean }>;
type TextInput = Readonly<{ handle: FileHandle; limit: number; preserveBom?: boolean }>;
type OpenInput = TextInput & Readonly<{ file: string; prior: Stats }>;

/** Combine Node open flags to avoid following links or blocking on a FIFO. */
// oxlint-disable-next-line no-bitwise
const OPEN_FLAGS = constants.O_RDONLY | constants.O_NONBLOCK | constants.O_NOFOLLOW;

function decodeText({ buffer, used, limit, preserveBom = false }: DecodeInput): string {
  const data = buffer.subarray(0, used);

  if (used > limit || data.includes(0)) {
    throw new CheckerError({ message: 'incomplete text input.' });
  }

  // ast-grep-ignore: acronym-names-typescript -- The WHATWG TextDecoderOptions dictionary spells this member ignoreBOM.
  const decoder = new TextDecoder('utf-8', { fatal: true, ignoreBOM: preserveBom });

  return decoder.decode(data);
}

async function readText({ handle, limit, preserveBom }: TextInput): Promise<string> {
  const buffer = Buffer.alloc(limit + 1);
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

  return decodeText({ buffer, limit, preserveBom, used });
}

async function readOpened(input: OpenInput): Promise<string> {
  const { handle, prior, file } = input;
  const stat = await handle.stat();

  if (stat.dev !== prior.dev || stat.ino !== prior.ino || (await realpath(file)) !== file) {
    throw new CheckerError({ message: 'input changed while opening it.' });
  }

  return await readText(input);
}

/** Read a limited UTF-8 regular file and reject changed files, FIFOs, symlinks and binary content. */
export async function readRegular(input: ReadInput): Promise<string> {
  const file = resolveInside(input);
  const prior = await lstat(file);

  if (!prior.isFile() || prior.size > input.limit || (await realpath(file)) !== file) {
    throw new CheckerError({ message: 'input unavailable.' });
  }

  const handle = await open(file, OPEN_FLAGS);

  try {
    return await readOpened({
      file,
      handle,
      limit: input.limit,
      preserveBom: input.preserveBom,
      prior,
    });
  } finally {
    await handle.close();
  }
}
