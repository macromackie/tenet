import { strict as assert } from 'node:assert';
import { env } from 'node:process';
import { test, type TestContext } from 'node:test';
import { Option } from 'effect';
import { explainFindings, HttpMethod, OpenRouterTurn, ROUTER_BASE } from '../fx/index.mts';
import type { Finding } from '../types.d.mts';
import { contract } from './test-project.mts';

const ENDPOINT = `${ROUTER_BASE}/chat/completions`;
const REPORT_REQUEST = { body: '{"model":"fixture","messages":[]}', method: HttpMethod.Post };
const STREAM = 'data: {"choices":[]}\n\ndata: {"usage":{"cost":0.001}}\n\ndata: [DONE]\n\n';
const RETURNED_COST = 0.001;
const REQUEST = { init: REPORT_REQUEST, url: ENDPOINT };

function buildExplainStream(content: string): string {
  const delta = JSON.stringify({ choices: [{ delta: { content }, index: 0 }] });

  const finish = JSON.stringify({
    choices: [{ delta: {}, finish_reason: 'stop', index: 0 }],
    usage: { cost: RETURNED_COST },
  });

  return `data: ${delta}\n\ndata: ${finish}\n\ndata: [DONE]\n\n`;
}

const SAMPLE_ROWS: readonly Finding[] = [
  {
    check: 'ok-check',
    contract: contract.name,
    cost_usd: 0,
    message: 'Passed check.',
    outcome: 'passed',
    requested: false,
    reused: false,
    subject: 'src/a.ts',
  },
  {
    check: 'bad-check',
    contract: contract.name,
    cost_usd: 0,
    message: 'Expected good (expected true, got false)',
    outcome: 'failed',
    requested: false,
    reused: false,
    subject: 'src/a.ts',
  },
];

type FixtureExplainInput = Readonly<{
  onRequest: (cost: Finding['cost_usd']) => void;
  rows: readonly Finding[];
}>;

function canSpendBudget(): boolean {
  return true;
}

function ignoreCost(): void {
  // No-op when provider declines.
}

const EXPLAINED_MESSAGE = 'src/a.ts returned false instead of true, violating the contract.';

const EXPLAIN_PAYLOAD = JSON.stringify({
  explanations: {
    '0': 'Should never overwrite a passed row.',
    '1': EXPLAINED_MESSAGE,
  },
});

const EXPLAIN_SSE = buildExplainStream(EXPLAIN_PAYLOAD);

async function runExplainWithFixtureKey({
  onRequest,
  rows,
}: FixtureExplainInput): Promise<readonly Finding[]> {
  const previousKey = env.OPENROUTER_API_KEY;

  env.OPENROUTER_API_KEY = 'fixture';

  try {
    return await explainFindings({
      canSpendRequest: canSpendBudget,
      contract,
      onRequest,
      rows,
    });
  } finally {
    env.OPENROUTER_API_KEY = previousKey ?? '';
  }
}

void test('one provider response supplies the original stream and charge', async (context: TestContext) => {
  const provider = context.mock.method(
    globalThis,
    'fetch',
    () => new Response(STREAM, { headers: { 'content-type': 'text/event-stream' } }),
  );

  const router = new OpenRouterTurn('fixture');
  const response = await router.send(REQUEST);
  const text = await response.text();

  assert.equal(text, STREAM);
  assert.equal(router.cost, RETURNED_COST);

  const retry = router.send(REQUEST);

  await assert.rejects(retry, /one OpenRouter request/v);

  const calls = provider.mock.callCount();

  assert.equal(calls, 1);
});

void test('a foreign endpoint receives no request and a provider refusal keeps its HTTP status', async (context: TestContext) => {
  const provider = context.mock.method(
    globalThis,
    'fetch',
    () => new Response(null, { status: 402 }),
  );

  const router = new OpenRouterTurn('fixture');
  const foreign = router.send({ init: REPORT_REQUEST, url: 'https://example.invalid' });

  await assert.rejects(foreign, /one OpenRouter request/v);

  const outgoing = provider.mock.callCount();

  assert.equal(outgoing, 0);

  const refusal = router.send({ init: REPORT_REQUEST, url: ENDPOINT });

  await assert.rejects(refusal, /OpenRouter/v);

  const message = Option.getOrNull(router.failure);

  assert.equal(message, 'OpenRouter declined checker generation (HTTP 402)');
});

void test('fx finding explainer rewrites failures while preserving passed rows and outcomes', async (context: TestContext) => {
  let charged = 0;

  function recordCost(cost: Finding['cost_usd']): void {
    charged += cost ?? 0;
  }

  context.mock.method(
    globalThis,
    'fetch',
    () => new Response(EXPLAIN_SSE, { headers: { 'content-type': 'text/event-stream' } }),
  );

  const updated = await runExplainWithFixtureKey({
    onRequest: recordCost,
    rows: SAMPLE_ROWS,
  });

  assert.equal(updated[0]?.outcome, 'passed');
  assert.equal(updated[0]?.message, 'Passed check.');
  assert.equal(updated[1]?.outcome, 'failed');
  assert.equal(updated[1]?.message, EXPLAINED_MESSAGE);
  assert.equal(charged, RETURNED_COST);
});

void test('fx finding explainer falls back to raw diagnostics on provider refusal', async (context: TestContext) => {
  context.mock.method(globalThis, 'fetch', () => new Response(null, { status: 500 }));

  const updated = await runExplainWithFixtureKey({
    onRequest: ignoreCost,
    rows: SAMPLE_ROWS,
  });

  assert.deepEqual(updated, SAMPLE_ROWS);
});
