import { readFile } from 'node:fs/promises';
import { env } from 'node:process';
import { Effect, Option, Schema } from 'effect';
import type { FxAgent, FxFetch, FxTurn } from 'libfx';
import { CheckerError } from './checker-error.mts';
import { readLibrary } from './fx-library.mts';
import { OpenRouterTurn, ROUTER_BASE } from './openrouter.mts';
import { tryOperation } from './try-operation.mts';
import type { Contract, Finding, Inventory } from './types.d.mts';

const TURN_TIMEOUT_MS = 95_000;
const CONTEXT_WINDOW = 128_000;
const CheckerObject = Schema.Record(Schema.String, Schema.Unknown);
const CheckerJson = Schema.fromJsonString(CheckerObject);
const decodeJson = Schema.decodeUnknownOption(CheckerJson, { onExcessProperty: 'error' });

type GenerateInput = {
  readonly contract: Contract;
  readonly inventory: Inventory;
  readonly onRequest: (cost: Finding['cost_usd']) => void;
};

function encodeInput(request: GenerateInput): string {
  const { contract, inventory } = request;
  // ast-grep-ignore: object-from-entries-typescript -- The fx JSON prompt names each file by its project path.
  const files = Object.fromEntries(inventory.files);

  return JSON.stringify({
    applicability_hints: inventory.decisions,
    contract: contract.document,
    files,
    mechanical: contract.mechanical,
    previously_checked: contract.checks,
    reference_samples: inventory.assets,
  });
}

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

async function createAgent(router: OpenRouterTurn): Promise<FxAgent> {
  const { createFxAgent, memory } = await readLibrary();
  const promptUrl = new URL('checker-prompt.md', import.meta.url);
  const instructions = await readFile(promptUrl, 'utf-8');
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

type RunGenerationInput = {
  readonly agent: FxAgent;
  readonly router: OpenRouterTurn;
  readonly request: GenerateInput;
};

async function runGeneration({ agent, router, request }: RunGenerationInput): Promise<unknown> {
  const source = encodeInput(request);

  function fail(cause: unknown) {
    const message = Option.getOrElse(router.failure, () => 'could not return a usable checker');

    return new CheckerError({ cause, message });
  }

  async function run(signal: AbortSignal) {
    const session = agent.session();
    const turn = session.prompt(source, { signal });

    return await readAnswer(turn);
  }

  async function finish() {
    if (router.attempted) {
      request.onRequest(router.cost);
    }

    await agent.close();
  }

  const operation = Effect.tryPromise({ catch: fail, try: run });
  const close = tryOperation({ message: 'close fx generation', run: finish });
  const release = Effect.orDie(close);
  const result = operation.pipe(Effect.timeout(TURN_TIMEOUT_MS), Effect.ensuring(release));

  return await Effect.runPromise(result);
}

/** Generate observations for the current repository and charge at most one provider request. */
export async function generate(request: GenerateInput): Promise<unknown> {
  const key = readCredentials();
  const router = new OpenRouterTurn(key);
  const agent = await createAgent(router);

  return await runGeneration({ agent, request, router });
}
