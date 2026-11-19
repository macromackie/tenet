import { strict as assert } from 'node:assert';
import { test, type TestContext } from 'node:test';
import { Option } from 'effect';
import { HttpMethod } from '../http-method.mts';
import { OpenRouterTurn, ROUTER_BASE } from '../openrouter.mts';

const ENDPOINT = `${ROUTER_BASE}/chat/completions`;
const REPORT_REQUEST = { body: '{"model":"fixture","messages":[]}', method: HttpMethod.Post };
const STREAM = 'data: {"choices":[]}\n\ndata: {"usage":{"cost":0.001}}\n\ndata: [DONE]\n\n';
const RETURNED_COST = 0.001;
const REQUEST = { init: REPORT_REQUEST, url: ENDPOINT };

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
