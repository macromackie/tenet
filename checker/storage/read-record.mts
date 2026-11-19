import { Option, Result } from 'effect';
import { classifyResult } from '../classify-result.mts';
import { readRegular } from './read-regular.mts';
import type { LocationInput } from './resolve-inside.mts';

const RECORD_LIMIT = 2_097_152;

export type RecordData = { readonly text: string; readonly json: unknown };

function parseRecord(text: string): RecordData {
  const contents = text.replace(/^\uFEFF/v, '');
  const json: unknown = JSON.parse(contents);

  return { json, text };
}

/** Read exact bounded UTF-8 record contents, distinguishing a missing file from I/O failure. */
export async function readRecord(location: LocationInput): Promise<Option.Option<RecordData>> {
  async function run() {
    return await readRegular({ ...location, limit: RECORD_LIMIT, preserveBom: true });
  }

  const status = await classifyResult({ message: 'could not read checker state', run });

  if (Result.isSuccess(status)) {
    const record = parseRecord(status.success);

    return Option.some(record);
  }

  const { cause } = status.failure;

  if (cause instanceof Error && 'code' in cause && cause.code === 'ENOENT') {
    return Option.none();
  }

  throw status.failure;
}
