import { text } from 'node:stream/consumers';
import { Schema, Option } from 'effect';
import { CheckerError } from './checker-error.mts';
import { run } from './review/index.mts';
import { Request } from './wire.mts';

const decodeRequest = Schema.decodeUnknownOption(Request, { onExcessProperty: 'error' });

async function main(): Promise<void> {
  const requestText = await text(process.stdin);
  const input: unknown = JSON.parse(requestText);
  const request = decodeRequest(input);

  if (Option.isNone(request)) {
    throw new CheckerError({ message: 'invalid checker request' });
  }

  const rows = await run(request.value);

  // The host reads this worker's single JSON result from its own output file.
  const resultText = JSON.stringify(rows);

  // oxlint-disable-next-line no-restricted-properties
  process.stdout.write(resultText);
}

// This command has no module consumers; the Rust host owns its process lifetime.
// oxlint-disable-next-line node/no-top-level-await
await main();
