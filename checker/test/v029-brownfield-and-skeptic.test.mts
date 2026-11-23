import { strict as assert } from 'node:assert';
import { mkdir, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { test, type TestContext } from 'node:test';
import { readInventory } from '../discovery/index.mts';
import { type CandidateEvaluation, qualifyBundle } from '../review/index.mts';
import { observe, validate } from '../sandbox/index.mts';
import { canRun } from './can-run-sandbox.mts';
import { contract, createProject } from './test-project.mts';

const BUGGY_PY = 'def double_positive(x):\n    if x == 0:\n        return -1\n    return x * 2\n';

const PROPERTY_PROBE = [
  'import json',
  'from app import double_positive',
  'passed = 0',
  'for n in [1, 2, 3, 4, 5, 0]:',
  '    if double_positive(n) != n * 2:',
  '        print(json.dumps({"ok": False, "passed": passed, "case": n}))',
  '        raise SystemExit(0)',
  '    passed += 1',
  'print(json.dumps({"ok": True}))',
].join('\n');

async function verifyBrownfieldLifecycle(): Promise<void> {
  const buggyFiles = new Map<string, string>([['app.py', BUGGY_PY]]);

  const candidate = {
    defect: [],
    expected: { ok: true },
    good: [{ find: 'return -1', replace: 'return 0' }],
    id: 'zero-doubles-to-zero',
    promise: 'double_positive(0) returns 0.',
    runtime: 'python',
    subject: 'app.py',
    test: PROPERTY_PROBE,
  };

  const check = validate({ check: candidate, files: buggyFiles });
  const onBuggy = await observe({ check, files: buggyFiles, qualify: true });
  const fixedFiles = new Map<string, string>([['app.py', check.good]]);
  const revalidated = validate({ check, files: fixedFiles, fromPin: true });
  const onFixed = await observe({ check: revalidated, files: fixedFiles, qualify: true });

  assert.deepEqual(
    [check.brownfield, check.refactor === check.good, onBuggy.passed, onFixed.passed],
    [true, true, false, true],
  );
}

void test('brownfield Layer 1 qualifies omitted refactor and replays pinned checks after fix', async (context: TestContext) => {
  if (!(await canRun())) {
    context.skip('Isolated sandbox is unavailable');

    return;
  }

  await verifyBrownfieldLifecycle();
});

function buildUnpatchableBundle(): Readonly<Record<string, unknown>> {
  return {
    checks: [
      {
        defect: [],
        expected: { ok: true },
        good: [{ find: 'nonexistent_anchor_line', replace: 'return 0' }],
        id: 'salvaged-witness',
        promise: 'double_positive(0) returns 0.',
        refactor: [{ find: 'nonexistent_anchor_line', replace: 'return 0' }],
        runtime: 'python',
        subject: 'app.py',
        test: PROPERTY_PROBE,
      },
    ],
    open: [],
  };
}

type Layer2PairInput = Readonly<{
  auditMode: CandidateEvaluation;
  strict: CandidateEvaluation;
}>;

function assertLayer2Pair({ auditMode, strict }: Layer2PairInput): void {
  const strictFirst = strict.extraUnresolved.at(0);
  const auditFirst = auditMode.extraUnresolved.at(0);

  assert.deepEqual(
    [strict.qualified.length, strictFirst?.outcome, strictFirst?.brownfield_unverified],
    [0, 'unresolved', undefined],
  );
  assert.deepEqual(
    [
      auditMode.qualified.length,
      auditFirst?.outcome,
      auditFirst?.brownfield_unverified,
      auditFirst?.repro?.runtime,
    ],
    [0, 'failed', true, 'python'],
  );
}

async function verifyLayer2Salvage(root: string): Promise<void> {
  const appPath = join(root, 'app.py');

  await writeFile(appPath, BUGGY_PY);

  const scoped = { ...contract, paths: ['app.py'] };
  const current = await readInventory({ contract: scoped, root });
  const value = buildUnpatchableBundle();
  const strict = await qualifyBundle({ contractName: 'demo', current, value });

  const auditMode = await qualifyBundle({
    allowBrownfieldWitness: true,
    contractName: 'demo',
    current,
    value,
  });

  assertLayer2Pair({ auditMode, strict });
}

void test('brownfield Layer 2 salvages failing witnesses only when allowBrownfieldWitness is enabled', async (context: TestContext) => {
  if (!(await canRun())) {
    context.skip('Isolated sandbox is unavailable');

    return;
  }

  const root = await createProject(context);

  await verifyLayer2Salvage(root);
});

async function writeLocalPkgFixture(root: string): Promise<void> {
  const pkgDir = join(root, 'localpkg');
  const initPath = join(pkgDir, '__init__.py');
  const corePath = join(pkgDir, 'core.py');

  await mkdir(pkgDir, { recursive: true });
  await writeFile(initPath, 'from .core import val\n');
  await writeFile(corePath, 'def val():\n    return 1\n');
}

function buildMissingDepBundle(): Readonly<Record<string, unknown>> {
  return {
    checks: [
      {
        defect: [{ find: 'return 1', replace: 'return 0' }],
        expected: 1,
        good: [],
        id: 'external-dep',
        promise: 'Uses uninstalled matplotlib.',
        refactor: [{ find: 'return 1', replace: 'return int(True)' }],
        runtime: 'python',
        subject: 'localpkg/core.py',
        test: 'import matplotlib\nprint(1)\n',
      },
      {
        defect: [{ find: 'return 1', replace: 'return 0' }],
        expected: 1,
        good: [],
        id: 'local-typo',
        promise: 'Has repo-local import typo.',
        refactor: [{ find: 'return 1', replace: 'return int(True)' }],
        runtime: 'python',
        subject: 'localpkg/core.py',
        test: 'import localpkg.typo_module\nprint(1)\n',
      },
    ],
    open: [],
  };
}

async function verifyMissingDepPartition(root: string): Promise<void> {
  await writeLocalPkgFixture(root);

  const scoped = { ...contract, paths: ['localpkg/__init__.py', 'localpkg/core.py'] };
  const current = await readInventory({ contract: scoped, root });
  const value = buildMissingDepBundle();
  const result = await qualifyBundle({ contractName: 'deps', current, value });
  const unrecoverable = result.unrecoverableErrors ?? [];
  const hasMatplotlib = (unrecoverable.at(0) ?? '').includes('matplotlib');
  const hasLocalTypo = (result.errors.at(0) ?? '').includes('local-typo');

  assert.deepEqual(
    [unrecoverable.length, hasMatplotlib, result.errors.length, hasLocalTypo],
    [1, true, 1, true],
  );
}

void test('qualifyBundle fast-fails external missing deps while keeping repo-local import typos repairable', async (context: TestContext) => {
  if (!(await canRun())) {
    context.skip('Isolated sandbox is unavailable');

    return;
  }

  const root = await createProject(context);

  await verifyMissingDepPartition(root);
});
