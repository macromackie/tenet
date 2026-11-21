import { Buffer } from 'node:buffer';
import { Effect, Option, Schema } from 'effect';
import { CheckerError } from '../checker-error.mts';
import { tryOperation } from '../try-operation.mts';
import type { Finding } from '../types.d.mts';
import { HttpMethod } from './http-method.mts';

export type ReasoningEffort = 'low' | 'medium' | 'high';

type OpenRouterTurnOptions = Readonly<{
  key: string;
  reasoningEffort?: ReasoningEffort;
}>;

export const ROUTER_BASE = 'https://openrouter.ai/api/v1';
const CHAT_ENDPOINT = `${ROUTER_BASE}/chat/completions`;
const MAX_RESPONSE_BYTES = 2_097_152;
const MAX_REQUEST_BYTES = 1_048_576;
const REQUEST_TIMEOUT_MS = 110_000;
const OUTPUT_TOKENS = 16_384;
const JsonObject = Schema.Record(Schema.String, Schema.Unknown);
const JsonText = Schema.fromJsonString(JsonObject);
const decodeInput = Schema.decodeUnknownOption(JsonText, { onExcessProperty: 'error' });
const decodeRecord = Schema.decodeUnknownOption(JsonObject, { onExcessProperty: 'error' });
type SendInput = { readonly url: string; readonly init: RequestInit };

type BuildRequestInput = {
  readonly key: string;
  readonly init: RequestInit;
  readonly deadline: AbortSignal;
  readonly reasoningEffort: ReasoningEffort;
};

type PrepareBodyInput = Readonly<{
  reasoningEffort: ReasoningEffort;
  source: RequestInit['body'];
}>;

type Received = { readonly response: Response; readonly cost: Option.Option<number> };

function readUsageCost(value: unknown): Option.Option<number> {
  const usage = decodeRecord(value);

  if (Option.isNone(usage)) {
    return Option.none();
  }

  const { cost } = usage.value;

  if (typeof cost === 'number' && Number.isFinite(cost) && cost >= 0) {
    return Option.some(cost);
  }

  return Option.none();
}

function readCost(line: string): Option.Option<number> {
  if (!line.startsWith('data:')) {
    return Option.none();
  }

  const data = line.slice('data:'.length).trim();
  const packet = decodeInput(data);

  if (Option.isNone(packet)) {
    return Option.none();
  }

  return readUsageCost(packet.value.usage);
}

async function collectBytes(body: NonNullable<Response['body']>): Promise<Buffer> {
  const chunks: Uint8Array[] = [];
  let length = 0;

  for await (const raw of body) {
    if (!(raw instanceof Uint8Array)) {
      throw new CheckerError({ message: 'received an invalid stream from OpenRouter' });
    }

    length += raw.byteLength;

    if (length > MAX_RESPONSE_BYTES) {
      throw new CheckerError({ message: 'received an oversized response from OpenRouter' });
    }

    chunks.push(raw);
  }

  return Buffer.concat(chunks);
}

function readRequestText(body: RequestInit['body']): string {
  if (typeof body === 'string' && Buffer.byteLength(body) <= MAX_REQUEST_BYTES) {
    return body;
  }

  if (body instanceof Uint8Array && body.byteLength <= MAX_REQUEST_BYTES) {
    return Buffer.from(body).toString('utf-8');
  }

  throw new CheckerError({ message: 'cannot fit the fx request into the checker context' });
}

function prepareBody({ reasoningEffort, source }: PrepareBodyInput): string {
  const text = readRequestText(source);
  const decoded = decodeInput(text);

  if (Option.isNone(decoded)) {
    throw new CheckerError({ message: 'received invalid request JSON from fx' });
  }

  return JSON.stringify({
    ...decoded.value,
    max_tokens: OUTPUT_TOKENS,
    reasoning: { effort: reasoningEffort },
    response_format: { type: 'json_object' },
    stream_options: { include_usage: true },
  });
}

function buildRequest({ key, init, deadline, reasoningEffort }: BuildRequestInput): RequestInit {
  const signals = [deadline];

  if (init.signal) {
    signals.push(init.signal);
  }

  const body = prepareBody({ reasoningEffort, source: init.body });
  const signal = AbortSignal.any(signals);

  return {
    body,
    headers: { authorization: `Bearer ${key}`, 'content-type': 'application/json' },
    method: HttpMethod.Post,
    redirect: 'error',
    signal,
  };
}

function readReturnedCost(text: string): Option.Option<number> {
  let cost = Option.none<number>();

  for (const line of text.split('\n')) {
    const next = readCost(line);

    if (Option.isSome(next)) {
      cost = next;
    }
  }

  return cost;
}

async function readResponse(response: Response): Promise<Received> {
  if (response.body === null) {
    throw new CheckerError({ message: 'received an empty OpenRouter response' });
  }

  const bytes = await collectBytes(response.body);
  const text = bytes.toString('utf-8');
  const cost = readReturnedCost(text);
  const headers = { 'content-type': response.headers.get('content-type') ?? 'text/event-stream' };

  return { cost, response: new Response(text, { headers, status: response.status }) };
}

/** Own one turn's provider request, returned cost and sanitized rejection reason. */
export class OpenRouterTurn {
  readonly #key: string;
  readonly #reasoningEffort: ReasoningEffort;
  #attempted = false;
  #cost = Option.none<number>();
  #failure = Option.none<string>();

  constructor(input: string | OpenRouterTurnOptions) {
    if (typeof input === 'string') {
      this.#key = input;
      this.#reasoningEffort = 'low';

      return;
    }

    const { key, reasoningEffort = 'low' } = input;

    this.#key = key;
    this.#reasoningEffort = reasoningEffort;
  }

  get attempted(): boolean {
    return this.#attempted;
  }
  get cost(): Finding['cost_usd'] {
    return Option.getOrNull(this.#cost);
  }
  get failure(): Option.Option<string> {
    return this.#failure;
  }

  async #receive(response: Response): Promise<Response> {
    if (!response.ok) {
      const message = `OpenRouter declined checker generation (HTTP ${String(response.status)})`;
      this.#failure = Option.some(message);

      await response.body?.cancel();

      throw new CheckerError({ message });
    }

    const received = await readResponse(response);
    this.#cost = received.cost;

    return received.response;
  }

  /** Refuse any extra request even when the SDK retries after a provider error. */
  async send({ url, init }: SendInput): Promise<Response> {
    if (url !== CHAT_ENDPOINT || init.method !== HttpMethod.Post || this.#attempted) {
      throw new CheckerError({ message: 'the checker may send one OpenRouter request' });
    }

    const run = async (deadline: AbortSignal) => {
      const request = buildRequest({
        deadline,
        init,
        key: this.#key,
        reasoningEffort: this.#reasoningEffort,
      });

      this.#attempted = true;
      const response = await fetch(CHAT_ENDPOINT, request);

      return await this.#receive(response);
    };

    const download = tryOperation({ message: 'request the checker from OpenRouter', run });
    const bounded = download.pipe(Effect.timeout(REQUEST_TIMEOUT_MS));

    return await Effect.runPromise(bounded);
  }
}
