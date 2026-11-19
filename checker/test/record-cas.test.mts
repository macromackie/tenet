import { strict as assert } from 'node:assert';
import { mkdtemp, writeFile, readFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test, type TestContext } from 'node:test';
import { Option } from 'effect';
import { replacePin, readRecord } from '../storage/index.mts';

async function makeFixture(context: TestContext) {
  const directory = tmpdir();
  const prefix = join(directory, 'tenet-pin-cas-');
  const root = await mkdtemp(prefix);

  context.after(async () => await rm(root, { force: true, recursive: true }));

  const location = { path: 'checker.json', root };
  const target = join(root, location.path);

  await writeFile(target, '{}');

  const prior = await readRecord(location);

  return { location, prior, target };
}

void test('a concurrent byte-only edit to a valid JSON pin prevents replacement', async (context) => {
  const { location, prior, target } = await makeFixture(context);
  const hasPrior = Option.isSome(prior);

  assert.ok(hasPrior);

  const changed = '\uFEFF{}';

  await writeFile(target, changed);

  const replacing = replacePin({ ...location, prior, value: { next: true } });

  await assert.rejects(replacing, /pin changed/v);

  const after = await readFile(target, 'utf-8');

  assert.equal(after, changed);
});
