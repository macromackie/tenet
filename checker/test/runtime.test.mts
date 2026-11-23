import { strict as assert } from 'node:assert';
import { cp, mkdir, writeFile } from 'node:fs/promises';
import { dirname, join } from 'node:path';
import { test, type TestContext } from 'node:test';
import { readInventory } from '../discovery/index.mts';
import { run } from '../review/index.mts';
import type { Contract, Finding } from '../types.d.mts';
import { canRun } from './can-run-sandbox.mts';
import { commitBaseline } from './commit-baseline.mts';
import { check, contract, createProject, GOOD, lock, SOURCE } from './test-project.mts';

type AssessInput = {
  root: string;
  selected?: Contract;
  inspect?: boolean;
  fresh?: boolean;
  base?: string;
};

async function assess({
  root,
  selected = contract,
  inspect = false,
  fresh = false,
  base,
}: AssessInput): Promise<readonly Finding[]> {
  const state = join(root, '.tenet');

  return await run({
    base,
    contracts: [selected],
    fresh,
    inspect,
    maxGenerations: 0,
    root,
    state,
  });
}

async function writeLargeSource(root: string) {
  const large = 'large.ts';
  const bytes = 131_073;

  const contents = 'x'.repeat(bytes);
  const largePath = join(root, large);

  await writeFile(largePath, contents);

  return { ...contract, paths: [SOURCE, large] };
}

void test('retains unresolved coverage after a model-free check', async (context) => {
  const root = await createProject(context);
  const selected = await writeLargeSource(root);
  const rows = await assess({ root, selected });

  const hasGap = rows.some((row) => row.check === 'coverage' && row.outcome === 'unresolved');

  assert.ok(hasGap);

  const inspected = await assess({ inspect: true, root, selected });

  const hasSavedGap = inspected.some((row) => row.check === 'coverage');

  assert.ok(hasSavedGap);
});

type InstallPinInput = { root: string; open?: readonly string[]; label?: string };

async function installPin({
  root,
  open = [],
  label = check.promise,
}: InstallPinInput): Promise<void> {
  const { shape } = await readInventory({ contract, root });
  const pin = { bundle: { checks: [{ ...check, promise: label }], open }, protocol: 1, shape };

  const serializedPin = JSON.stringify(pin);
  const lockPath = join(root, lock);
  const lockDir = dirname(lockPath);

  await mkdir(lockDir, { recursive: true });
  await writeFile(lockPath, serializedPin);
}

async function preparePinned(context: TestContext) {
  const root = await createProject(context);

  await installPin({ root });

  return root;
}

function assertPinReplay(row: Finding): void {
  assert.equal(row.reused, false);
  assert.equal(row.pin_reused, true);
  assert.equal(row.requested, false);
}

async function verifyReuse(root: string) {
  const [cold] = await assess({ root });
  const [warm] = await assess({ root });

  assert.equal(cold.outcome, 'failed', cold.message);
  assertPinReplay(cold);
  assert.equal(warm.reused, true);

  const [fresh] = await assess({ fresh: true, root });

  assertPinReplay(fresh);
}

type CopyPinLockInput = Readonly<{ from: string; to: string }>;

async function copyPinLock({ from, to }: CopyPinLockInput): Promise<void> {
  const cloneLockPath = join(to, lock);
  const cloneLockDir = dirname(cloneLockPath);
  const lockPath = join(from, lock);

  await mkdir(cloneLockDir, { recursive: true });
  await cp(lockPath, cloneLockPath);
}

async function replayOnClone(context: TestContext): Promise<string> {
  const root = await preparePinned(context);

  await verifyReuse(root);

  const clean = await createProject(context);

  await copyPinLock({ from: root, to: clean });

  const [fresh] = await assess({ root: clean });

  assertPinReplay(fresh);

  return clean;
}

async function writeFix(root: string) {
  const sourcePath = join(root, SOURCE);

  await writeFile(sourcePath, GOOD);
}

void test('a fresh checkout distinguishes a pre-existing failure from a PR regression', async (context) => {
  if (!(await canRun())) {
    context.skip('Linux bubblewrap is unavailable');

    return;
  }

  const root = await replayOnClone(context);
  const base = await commitBaseline(root);

  const [prior] = await assess({ base, root });

  assert.equal(prior.outcome, 'unresolved', prior.message);

  await writeFix(root);

  const [fixed] = await assess({ root });

  assert.equal(fixed.outcome, 'passed', fixed.message);
});

async function verifyModifiedPin(root: string) {
  const open = ['Unchecked edge case'];

  await installPin({ open, root });

  const measured = await assess({ root });
  const hadOpen = measured.some((row) => row.message === 'Unchecked edge case');

  assert.ok(hadOpen);
  await installPin({ label: 'A different promise', open, root });

  const inspected = await assess({ inspect: true, root });
  const stale = inspected.some((row) => row.outcome === 'failed' || row.reused);

  assert.equal(stale, false);
}

void test('editing a partly unresolved pin invalidates its evidence in show', async (context) => {
  if (!(await canRun())) {
    context.skip('Linux bubblewrap is unavailable');

    return;
  }

  const root = await createProject(context);

  await verifyModifiedPin(root);
});

async function writePackage(root: string) {
  const folder = join(root, 'pkg');

  await mkdir(folder);

  const entry = join(folder, '__init__.py');
  const table = join(folder, 'units.txt');
  const other = join(root, 'notes.txt');

  await writeFile(entry, '');
  await writeFile(table, 'meters');
  await writeFile(other, 'unrelated');

  return table;
}

function verifyPackage(initial: Awaited<ReturnType<typeof readInventory>>) {
  const data = initial.runtimeFiles.get('pkg/units.txt');

  assert.equal(data, 'meters');

  const prompted = initial.files.has('pkg/units.txt');

  assert.equal(prompted, false);

  const included = initial.runtimeFiles.has('notes.txt');

  assert.equal(included, false);
}

void test('scoped Python package data affects replay without entering the model packet', async (context) => {
  const root = await createProject(context);
  const table = await writePackage(root);
  const selected = { ...contract, paths: ['pkg/__init__.py', 'pkg/units.txt', 'notes.txt'] };
  const input = { contract: selected, root };
  const initial = await readInventory(input);

  verifyPackage(initial);
  await writeFile(table, 'feet');

  const changed = await readInventory(input);

  assert.notEqual(initial.observed, changed.observed);
});
