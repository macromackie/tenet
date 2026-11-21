import { env } from 'node:process';
import { Effect, Option, Schema } from 'effect';
import type { FxAgent, FxFetch, FxTurn } from 'libfx';
import { CheckerError } from '../checker-error.mts';
import { tryOperation } from '../try-operation.mts';
import type { Finding } from '../types.d.mts';
import { readLibrary } from './fx-library.mts';
import { OpenRouterTurn, ROUTER_BASE } from './openrouter.mts';

const TURN_TIMEOUT_MS = 120_000;
const CONTEXT_WINDOW = 128_000;
const CONTEXT_LIMIT_PATTERN = /context|token|window|too long|exceeds/iv;
const CheckerObject = Schema.Record(Schema.String, Schema.Unknown);
const CheckerJson = Schema.fromJsonString(CheckerObject);
const decodeJson = Schema.decodeUnknownOption(CheckerJson, { onExcessProperty: 'error' });

function decodeAnswer(text: string): Readonly<Record<string, unknown>> {
  const content = text.trim().replaceAll(/^```(?:json)?\s*|\s*```$/gv, '');
  const decoded = decodeJson(content);

  if (Option.isNone(decoded)) {
    throw new CheckerError({ message: 'received invalid checker JSON' });
  }

  return decoded.value;
}

async function readAnswer(turn: FxTurn): Promise<unknown> {
  let text = '';

  for await (const event of turn) {
    if (event.type === 'text_delta') {
      text += event.delta;
    }
  }

  const result = await turn.result;

  if (result.error || result.stopReason === 'refused') {
    throw new CheckerError({ message: 'could not complete checker generation' });
  }

  return decodeAnswer(text);
}

function readCredentials(): string {
  const key = env.OPENROUTER_API_KEY;

  if (key === undefined || key.length === 0) {
    throw new CheckerError({ message: 'set OPENROUTER_API_KEY to build the checker' });
  }

  return key;
}

type CreateAgentInput = Readonly<{
  instructions: string;
  router: OpenRouterTurn;
}>;

async function createAgent({ instructions, router }: CreateAgentInput): Promise<FxAgent> {
  const { createFxAgent, memory } = await readLibrary();
  const model = env.TENET_AGENT_MODEL ?? 'openai/gpt-6-luna';
  const durability = memory();
  // Q: libfx supplies URL and request options separately to its fetch hook.
  // oxlint-disable-next-line max-params
  const send: FxFetch = async (url, init) => await router.send({ init, url });

  const settings = {
    durability,
    // ast-grep-ignore: lookup-names-typescript -- The libfx host-fetch option is named fetch.
    fetch: send,
    instructions,
    model,
    provider: {
      baseUrl: ROUTER_BASE,
      models: { [model]: { contextWindow: CONTEXT_WINDOW, supportsToolUse: false } },
      name: 'tenet',
    },
    tools: [],
  };

  return createFxAgent(settings);
}

type RunTurnInput = Readonly<{
  agent: FxAgent;
  onRequest: (cost: Finding['cost_usd']) => void;
  router: OpenRouterTurn;
  source: string;
  timeoutMs: number;
}>;

function formatTurnFailure(cause: unknown): string {
  if (cause instanceof Error && CONTEXT_LIMIT_PATTERN.test(cause.message)) {
    return "governed source exceeded the checker context window; narrow the contract's paths (for example excluding testdata or *_test files) and rerun";
  }

  return 'could not return a usable checker';
}

async function runTurn({
  agent,
  onRequest,
  router,
  source,
  timeoutMs,
}: RunTurnInput): Promise<unknown> {
  function fail(cause: unknown) {
    const message = Option.getOrElse(router.failure, () => formatTurnFailure(cause));

    return new CheckerError({ cause, message });
  }

  async function run(signal: AbortSignal) {
    const session = agent.session();
    const turn = session.prompt(source, { signal });

    return await readAnswer(turn);
  }

  async function finish() {
    if (router.attempted) {
      onRequest(router.cost);
    }

    await agent.close();
  }

  const operation = Effect.tryPromise({ catch: fail, try: run });
  const close = tryOperation({ message: 'close fx generation', run: finish });
  const release = Effect.orDie(close);
  const result = operation.pipe(Effect.timeout(timeoutMs), Effect.ensuring(release));

  return await Effect.runPromise(result);
}

type FxJsonInput = Readonly<{
  instructions: string;
  onRequest: (cost: Finding['cost_usd']) => void;
  source: string;
  timeoutMs?: number;
}>;

/** Execute one bounded JSON-returning fx turn against OpenRouter. */
export async function runFxJson({
  instructions,
  onRequest,
  source,
  timeoutMs = TURN_TIMEOUT_MS,
}: FxJsonInput): Promise<unknown> {
  const key = readCredentials();
  const router = new OpenRouterTurn(key);
  const agent = await createAgent({ instructions, router });

  return await runTurn({ agent, onRequest, router, source, timeoutMs });
}
