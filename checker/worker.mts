import { text } from 'node:stream/consumers';
import { Option, Schema } from 'effect';
import {
  AuditFixRequest,
  AuditReplayRequest,
  AuditRequest,
  runAudit,
  runAuditFix,
  runAuditReplay,
} from './audit/index.mts';
import { CheckerError } from './checker-error.mts';
import { run } from './review/index.mts';
import { Request } from './wire.mts';

const decodeRequest = Schema.decodeUnknownOption(Request, { onExcessProperty: 'error' });
const decodeAuditRequest = Schema.decodeUnknownOption(AuditRequest, { onExcessProperty: 'error' });

const decodeReplayRequest = Schema.decodeUnknownOption(AuditReplayRequest, {
  onExcessProperty: 'error',
});

const decodeFixRequest = Schema.decodeUnknownOption(AuditFixRequest, {
  onExcessProperty: 'error',
});

function readMode(input: unknown): Option.Option<string> {
  if (input === null || typeof input !== 'object' || !('mode' in input)) {
    return Option.none();
  }

  if (typeof input.mode === 'string') {
    return Option.some(input.mode);
  }

  return Option.none();
}

async function handleAuditMode(input: unknown): Promise<unknown> {
  const auditRequest = decodeAuditRequest(input);

  if (Option.isNone(auditRequest)) {
    throw new CheckerError({ message: 'invalid audit request' });
  }

  return await runAudit(auditRequest.value);
}

async function handleReplayMode(input: unknown): Promise<unknown> {
  const replayRequest = decodeReplayRequest(input);

  if (Option.isNone(replayRequest)) {
    throw new CheckerError({ message: 'invalid replay request' });
  }

  return await runAuditReplay(replayRequest.value);
}

async function handleFixMode(input: unknown): Promise<unknown> {
  const fixRequest = decodeFixRequest(input);

  if (Option.isNone(fixRequest)) {
    throw new CheckerError({ message: 'invalid audit-fix request' });
  }

  return await runAuditFix(fixRequest.value);
}

async function handleReviewMode(input: unknown): Promise<unknown> {
  const request = decodeRequest(input);

  if (Option.isNone(request)) {
    throw new CheckerError({ message: 'invalid checker request' });
  }

  return await run(request.value);
}

async function handleInput(input: unknown): Promise<unknown> {
  const modeOption = readMode(input);
  const mode = Option.getOrUndefined(modeOption);

  if (mode === 'audit') {
    return await handleAuditMode(input);
  }

  if (mode === 'replay') {
    return await handleReplayMode(input);
  }

  if (mode === 'audit-fix') {
    return await handleFixMode(input);
  }

  return await handleReviewMode(input);
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
