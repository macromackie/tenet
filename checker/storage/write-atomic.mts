import type { LocationInput } from './resolve-inside.mts';
import { writeTextAtomic } from './write-text-atomic.mts';

export type WriteInput = LocationInput & Readonly<{ first?: boolean; value: unknown }>;

/** Install an immutable first pin or replace an observation as a single directory operation. */
export async function writeAtomic(input: WriteInput): Promise<void> {
  const content = JSON.stringify(input.value);

  await writeTextAtomic({
    first: input.first,
    path: input.path,
    root: input.root,
    text: `${content}\n`,
  });
}
