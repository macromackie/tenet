import { text } from 'node:stream/consumers';
import { Option, Schema } from 'effect';
import { AuditRequest, runAudit } from './audit/index.mts';
import { CheckerError } from './checker-error.mts';
import { run } from './review/index.mts';
import { Request } from './wire.mts';

const decodeRequest = Schema.decodeUnknownOption(Request, { onExcessProperty: 'error' });
const decodeAuditRequest = Schema.decodeUnknownOption(AuditRequest, { onExcessProperty: 'error' });

function isAuditMode(input: unknown): boolean {
  if (input === null || typeof input !== 'object') {
    return false;
  }

  return 'mode' in input && input.mode === 'audit';
}

async function handleInput(input: unknown): Promise<unknown> {
  if (isAuditMode(input)) {
    const auditRequest = decodeAuditRequest(input);

    if (Option.isNone(auditRequest)) {
      throw new CheckerError({ message: 'invalid audit request' });
    }

    return await runAudit(auditRequest.value);
  }

  const request = decodeRequest(input);

  if (Option.isNone(request)) {
    throw new CheckerError({ message: 'invalid checker request' });
  }

  return await run(request.value);
}

async function main(): Promise<void> {
  const requestText = await text(process.stdin);
  const input: unknown = JSON.parse(requestText);
  const result = await handleInput(input);

  // The host reads this worker's single JSON result from its own output file.
  const resultText = JSON.stringify(result);

  // oxlint-disable-next-line no-restricted-properties
  process.stdout.write(resultText);
}

// This command has no module consumers; the Rust host owns its process lifetime.
// oxlint-disable-next-line node/no-top-level-await
await main();
