import { strict as assert } from 'node:assert';
import { readFile, writeFile } from 'node:fs/promises';
import { test, type TestContext } from 'node:test';
import { Option } from 'effect';
import { readInventory } from '../../discovery/index.mts';
import { run, updatePin } from '../../review/index.mts';
import {
  hashSource,
  readPinScripts,
  readRecord,
  resolveScriptExtension,
  writeAtomic,
  writePinScripts,
} from '../../storage/index.mts';
import { canRun } from '../can-run-sandbox.mts';
import { check, contract, createProject, FOLDER, GOOD, lock, SOURCE } from '../test-project.mts';

const LEGACY_LOCK = `${FOLDER}/agent-checks.lock.json`;
const SCRIPT_PATH = `${FOLDER}/scripts/keeps-zero.mjs`;

const EDITED_FALSE_SCRIPT =
  'import {choose} from "./choose.mjs"; console.log(JSON.stringify(choose(false) === false ? 0 : 7));\n';

type PinCheck = Parameters<typeof readPinScripts>[0]['pin']['bundle']['checks'][number];
type MakePinInput = Readonly<{ checks: readonly PinCheck[]; contractHash?: string; shape?: string }>;

function makePin({ checks, contractHash = contract.hash, shape = 'shape-1' }: MakePinInput) {
  return { bundle: { checks, open: [] }, contractHash, protocol: 1 as const, shape };
}

function buildCollisionChecks(): readonly PinCheck[] {
  return [
    check,
    {
      ...check,
      id: 'keeps-zero',
      runtime: 'node',
      test: 'import {choose} from "./choose.mjs"; console.log(JSON.stringify(choose(1)));',
    },
    { ...check, id: 'py-check', runtime: 'python', test: 'print(1)\n' },
  ];
}

function assertScriptExtensions(): void {
  const nodeExt = resolveScriptExtension('node');
  const pythonExt = resolveScriptExtension('python');
  const goExt = resolveScriptExtension('go');
  const rustExt = resolveScriptExtension('rust');

  assert.deepEqual([nodeExt, pythonExt, goExt, rustExt], ['mjs', 'py', 'go', 'rs']);
}

async function readWrittenScripts(root: string): Promise<readonly [string, string, string]> {
  const firstScript = await readFile(`${root}/${SCRIPT_PATH}`, 'utf-8');
  const secondScript = await readFile(`${root}/${FOLDER}/scripts/keeps-zero-2.mjs`, 'utf-8');
  const thirdScript = await readFile(`${root}/${FOLDER}/scripts/py-check.py`, 'utf-8');

  return [firstScript, secondScript, thirdScript];
}

async function verifyMaterializedScripts(root: string): Promise<void> {
  const checks = buildCollisionChecks();
  const pin = makePin({ checks });
  const materialized = await writePinScripts({ lockPath: lock, pin, root });

  await writeAtomic({ path: lock, root, value: materialized.diskPin });

  const stored = await readRecord({ path: lock, root });
  const scripts = await readWrittenScripts(root);
  const [firstDisk, secondDisk, thirdDisk] = materialized.diskPin.bundle.checks;
  const hasStored = Option.isSome(stored);
  const expectedHash = hashSource(check.test);

  assert.deepEqual(
    [
      hasStored, firstDisk.scriptFile, firstDisk.scriptHash, firstDisk.test === undefined,
      secondDisk.scriptFile, thirdDisk.scriptFile, ...scripts,
      materialized.hydratedPin.bundle.checks[0].test,
    ],
    [
      true, 'keeps-zero.mjs', expectedHash, true, 'keeps-zero-2.mjs', 'py-check.py',
      check.test, checks[1].test, 'print(1)\n', check.test,
    ],
  );
}

void test('writePinScripts writes runnable scripts and compact scripts/.lock.json without inline test', async (context: TestContext) => {
  assertScriptExtensions();

  const root = await createProject(context);

  await verifyMaterializedScripts(root);
});

async function prepareHydrationFixture(root: string) {
  await writeFile(`${root}/${SOURCE}`, 'export const choose = (value) => (value === 0 ? 0 : value || 7);');

  const inventory = await readInventory({ contract, root });
  const pin = makePin({ checks: [check], contractHash: contract.hash, shape: inventory.shape });
  const materialized = await writePinScripts({ lockPath: lock, pin, root });

  await writeAtomic({ path: lock, root, value: materialized.diskPin });

  return materialized.diskPin;
}

async function assertReplayOutcome(
  input: Readonly<{ expected: 'passed' | 'failed'; reused?: boolean; root: string; state: string }>,
): Promise<void> {
  if (!(await canRun())) {
    return;
  }

  const rows = await run({ contracts: [contract], maxGenerations: 0, root: input.root, state: input.state });

  assert.equal(rows[0].outcome, input.expected);

  if (input.reused !== undefined) {
    assert.equal(rows[0].reused, input.reused);
  }
}

async function verifyHydratedReplay(root: string): Promise<void> {
  const diskPin = await prepareHydrationFixture(root);
  const clean = await readPinScripts({ lockPath: lock, pin: diskPin, root });
  const state = `${root}/.tenet`;

  assert.deepEqual([clean.handEditedIds.size, clean.pin.bundle.checks[0].test], [0, check.test]);
  await assertReplayOutcome({ expected: 'passed', root, state });
  await writeFile(`${root}/${SCRIPT_PATH}`, EDITED_FALSE_SCRIPT);

  const edited = await readPinScripts({ lockPath: lock, pin: diskPin, root });
  const isEdited = edited.handEditedIds.has('keeps-zero');

  assert.deepEqual([isEdited, edited.pin.bundle.checks[0].test], [true, EDITED_FALSE_SCRIPT]);
  await assertReplayOutcome({ expected: 'failed', reused: false, root, state });
}

void test('readPinScripts loads scripts from disk, detects manual edits, and replays edited code', async (context: TestContext) => {
  const root = await createProject(context);

  await verifyHydratedReplay(root);
});

function buildRegeneratedPin() {
  return makePin({
    checks: [
      {
        ...check,
        promise: 'Updated wording from LLM.',
        test: 'import {choose} from "./choose.mjs"; console.log(JSON.stringify(choose(99)));',
      },
    ],
    contractHash: 'new-contract-hash',
    shape: 'shape-2',
  });
}

async function prepareHandEditedPin(root: string) {
  const pin = makePin({ checks: [check] });
  const initial = await writePinScripts({ lockPath: lock, pin, root });
  const editedScript = 'import {choose} from "./choose.mjs"; console.log(JSON.stringify(42));\n';

  await writeFile(`${root}/${SCRIPT_PATH}`, editedScript);

  const hydratedPrior = await readPinScripts({ lockPath: lock, pin: initial.diskPin, root });

  return { editedScript, hydratedPrior, initial };
}

async function verifyHandEditGuard(root: string): Promise<void> {
  const { editedScript, hydratedPrior, initial } = await prepareHandEditedPin(root);
  const regeneratedPin = buildRegeneratedPin();
  const files = new Map<string, string>([[SOURCE, GOOD]]);

  const merged = updatePin({
    files,
    handEditedIds: hydratedPrior.handEditedIds,
    next: regeneratedPin,
    prior: hydratedPrior.pin,
  });

  const rewritten = await writePinScripts({
    lockPath: lock,
    pin: regeneratedPin,
    priorPin: initial.diskPin,
    root,
  });

  const onDiskAfter = await readFile(`${root}/${SCRIPT_PATH}`, 'utf-8');
  const isPreserved = rewritten.handEditedIds.has('keeps-zero');

  assert.deepEqual(
    [
      merged.bundle.checks[0].test, onDiskAfter, isPreserved,
      rewritten.diskPin.bundle.checks[0].scriptHash, rewritten.diskPin.bundle.checks[0].test === undefined,
      rewritten.hydratedPin.bundle.checks[0].test,
    ],
    [editedScript, editedScript, true, initial.diskPin.bundle.checks[0].scriptHash, true, editedScript],
  );
}

void test('hand-edit guard preserves edited scripts during writePinScripts and updatePin', async (context: TestContext) => {
  const root = await createProject(context);

  await verifyHandEditGuard(root);
});

async function verifyLegacyFallback(root: string): Promise<void> {
  const inventory = await readInventory({ contract, root });
  const legacyPin = makePin({ checks: [check], contractHash: contract.hash, shape: inventory.shape });
  const serializedLegacy = JSON.stringify(legacyPin);

  await writeFile(`${root}/${LEGACY_LOCK}`, serializedLegacy);

  const rows = await run({ contracts: [contract], maxGenerations: 0, root, state: `${root}/.tenet` });

  assert.deepEqual([rows.length, rows[0].outcome, rows[0].pin_reused], [1, 'failed', true]);
}

void test('legacy agent-checks.lock.json fallback still replays when scripts/.lock.json is absent', async (context: TestContext) => {
  if (!(await canRun())) {
    context.skip('Isolated sandbox is unavailable');

    return;
  }

  const root = await createProject(context);

  await verifyLegacyFallback(root);
});
