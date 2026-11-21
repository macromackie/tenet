import { strict as assert } from 'node:assert';
import { mkdir, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { test, type TestContext } from 'node:test';
import { listRankedPaths, readInventory } from '../discovery/index.mts';
import { qualifyBundle, run } from '../review/index.mts';
import { observe, validate } from '../sandbox/index.mts';
import type { Check, Contract, Pin } from '../types.d.mts';
import { canRun } from './can-run-sandbox.mts';
import { contract, createProject } from './test-project.mts';

const LONG_OPEN_CHARS = 520;
const MAX_OPEN_CHARS = 400;

type SingleOutcomeInput = Readonly<{
  expected: 'failed' | 'passed';
  inspect?: boolean;
  root: string;
  scoped: Contract;
}>;

function assertRustRanking(): void {
  const rustContract: Contract = {
    document: 'Ensure `Punctuated::insert` in `src/punctuated.rs` preserves trailing punctuation.',
    hash: 'h1',
    name: 'punctuated-insertion',
    path: '.contracts/001-punctuated-insertion/CONTRACT.md',
    paths: [
      'Cargo.toml',
      'src/gen/fold.rs',
      'src/gen/visit.rs',
      'src/attr.rs',
      'src/punctuated.rs',
      'src/tests/basic.rs',
    ],
  };

  const rankedRust = listRankedPaths(rustContract);
  const attrPos = rankedRust.indexOf('src/attr.rs');
  const genPos = rankedRust.indexOf('src/gen/fold.rs');

  assert.equal(rankedRust[0], 'Cargo.toml');
  assert.equal(rankedRust[1], 'src/punctuated.rs');
  assert.ok(attrPos < genPos);
}

function assertPythonRanking(): void {
  const pyContract: Contract = {
    document: 'Ensure import sorting preserves encoding declarations.',
    hash: 'h2',
    name: 'overwrite-encoding',
    path: '.contracts/002-overwrite-encoding/CONTRACT.md',
    paths: ['isort/_future/_dataclasses.py', 'isort/api.py', 'isort/core.py'],
  };

  const rankedPy = listRankedPaths(pyContract);
  const apiPos = rankedPy.indexOf('isort/api.py');
  const corePos = rankedPy.indexOf('isort/core.py');
  const futurePos = rankedPy.indexOf('isort/_future/_dataclasses.py');

  assert.ok(apiPos < futurePos);
  assert.ok(corePos < futurePos);
}

void test('discovery ranks contract-referenced modules ahead of generated and compat files', () => {
  assertRustRanking();
  assertPythonRanking();
});

async function assertNormalizedOpenItems(root: string): Promise<void> {
  const valuePath = join(root, 'value.ts');

  await writeFile(valuePath, 'export const value = 1;\n');

  const selected = { ...contract, paths: ['value.ts'] };
  const current = await readInventory({ contract: selected, root });
  const longNote = `Missing Cargo build artifacts: ${'a'.repeat(LONG_OPEN_CHARS)}`;

  const result = await qualifyBundle({
    contractName: 'syn',
    current,
    value: { checks: [], open: [longNote, { reason: 'Provide src/parse.rs for review.' }] },
  });

  assert.equal(result.errors.length, 0);
  assert.equal(result.open.length, 2);
  assert.ok((result.open[0]?.length ?? 0) <= MAX_OPEN_CHARS);
  assert.equal(result.open[1], 'Provide src/parse.rs for review.');
}

void test('decodePlannerBundle normalizes oversized and object open items instead of rejecting', async (context: TestContext) => {
  const root = await createProject(context);

  await assertNormalizedOpenItems(root);
});

void test('refactor-only control collision and failure emit targeted repair guidance', async (context: TestContext) => {
  if (!(await canRun())) {
    context.skip('Isolated sandbox is unavailable');

    return;
  }

  const files = new Map<string, string>([['sample.py', 'def f(x):\n    return 0\n']]);

  assert.throws(
    () =>
      validate({
        check: {
          defect: [],
          expected: 1,
          good: [{ find: 'return 0', replace: 'return 1' }],
          id: 'same-refactor',
          promise: 'f returns 1.',
          refactor: [{ find: 'return 0', replace: 'return 1' }],
          runtime: 'python',
          subject: 'sample.py',
          test: 'import sample\nprint(sample.f(0))\n',
        },
        files,
      }),
    /refactor matched good; rewrite refactor as a distinct behavior-preserving edit/v,
  );

  const badRefactor: Check = {
    defect: 'def f(x):\n    return 0\n',
    expected: 1,
    good: 'def f(x):\n    return 1\n',
    id: 'bad-refactor',
    promise: 'f returns 1.',
    refactor: 'def f(x):\n    return 2\n',
    runtime: 'python',
    subject: 'sample.py',
    test: 'import sample\nprint(sample.f(0))\n',
  };

  await assert.rejects(
    async () => await observe({ check: badRefactor, files, qualify: true }),
    /good and defect already passed/v,
  );
});

type BuildLocalPinInput = Readonly<{
  contractHash: string;
  shape: string;
}>;

function buildLocalPin({ contractHash, shape }: BuildLocalPinInput): Pin {
  return {
    bundle: {
      checks: [
        {
          defect: 'def val():\n    return 0\n',
          expected: 1,
          good: 'def val():\n    return 1\n',
          id: 'val-is-one',
          promise: 'val() returns 1.',
          refactor: 'def val():\n    return int(True)\n',
          runtime: 'python',
          subject: 'calc.py',
          test: 'import calc\nprint(calc.val())\n',
        },
      ],
      open: [],
    },
    contractHash,
    protocol: 1,
    shape,
  };
}

const LOCAL_RULE_CONTRACT: Contract = {
  document: 'val must return 1.',
  hash: 'contract-hash-1',
  name: 'rule',
  path: '.contracts/001-rule/CONTRACT.md',
  paths: ['calc.py'],
};

async function createPinnedLocalProject(root: string): Promise<Contract> {
  const contractDir = join(root, '.contracts', '001-rule');
  const calcPath = join(root, 'calc.py');
  const pinPath = join(contractDir, 'agent-checks.lock.json');

  await mkdir(contractDir, { recursive: true });
  await writeFile(calcPath, 'def val():\n    return 0\n');

  const inventory = await readInventory({ contract: LOCAL_RULE_CONTRACT, root });
  const pin = buildLocalPin({ contractHash: LOCAL_RULE_CONTRACT.hash, shape: inventory.shape });
  const serializedPin = JSON.stringify(pin);

  await writeFile(pinPath, serializedPin);

  return LOCAL_RULE_CONTRACT;
}

async function assertSingleOutcome({
  expected,
  inspect,
  root,
  scoped,
}: SingleOutcomeInput): Promise<void> {
  const state = join(root, '.tenet');

  const rows = await run({
    contracts: [scoped],
    inspect,
    maxGenerations: 0,
    root,
    state,
  });

  assert.equal(rows.length, 1);
  assert.equal(rows[0]?.outcome, expected);
}

async function assertReplayAndInspect({
  expected,
  root,
  scoped,
}: SingleOutcomeInput): Promise<void> {
  await assertSingleOutcome({ expected, root, scoped });
  await assertSingleOutcome({ expected, inspect: true, root, scoped });
}

void test('pinned checks replay across edits and preserve status receipts during inspect', async (context: TestContext) => {
  if (!(await canRun())) {
    context.skip('Isolated sandbox is unavailable');

    return;
  }

  const root = await createProject(context);
  const scoped = await createPinnedLocalProject(root);
  const calcPath = join(root, 'calc.py');

  await assertReplayAndInspect({ expected: 'failed', root, scoped });
  await writeFile(calcPath, 'def val():\n    return 1\n');
  await assertReplayAndInspect({ expected: 'passed', root, scoped });
});
