import { Option } from 'effect';
import { readRecord } from './read-record.mts';
import type { LocationInput } from './resolve-inside.mts';

/** Read the bounded JSON record when this checkout has already produced one. */
export async function readExisting(location: LocationInput): Promise<unknown> {
  const contents = await readRecord(location);

  if (Option.isSome(contents)) {
    return contents.value.json;
  }
}
