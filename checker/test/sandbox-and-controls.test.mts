import { strict as assert } from 'node:assert';
import { mkdir, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { test, type TestContext } from 'node:test';
import { readInventory } from '../discovery/index.mts';
import { qualifyBundle } from '../review/index.mts';
import { observe, validate } from '../sandbox/index.mts';
import type { Check } from '../types.d.mts';
import { canRun } from './can-run-sandbox.mts';
import { contract, createProject } from './test-project.mts';

const PADDING_LINES = 1_500;
const LARGE_MIN_BYTES = 65_536;
const LARGE_BODY = '// filler line preserving large file coverage\n'.repeat(PADDING_LINES);

const LARGE_SOURCE = `${LARGE_BODY}\nexport function resolveTimeout(value: number | undefined, readDefault: () => number): number {\n  if (!value) {\n    return readDefault();\n  }\n  return value;\n}`;

const LARGE_TEST =
  'import { resolveTimeout } from "./large.ts";\nlet reads = 0;\nconst zero = resolveTimeout(0, () => { reads += 1; return 30; });\nconst missing = resolveTimeout(undefined, () => { reads += 1; return 30; });\nconsole.log(JSON.stringify({ missing, reads, zero }));';

const PYTHON_SRC_INIT = 'from .calc import choose\n';

const PYTHON_SRC_BUGGY = 'def choose(v, fb):\n    if not v:\n        return fb()\n    return v\n';

const PYTHON_SRC_GOOD =
  'def choose(v, fb):\n    if v is None:\n        return fb()\n    return v\n';

const PYTHON_SRC_REFACTOR = 'def choose(v, fb):\n    return fb() if v is None else v\n';

const PYTHON_PROBE = [
  'import json',
  'from samplepkg import choose',
  'reads = 0',
  'def fb():',
  '    global reads',
  '    reads += 1',
  '    return 30',
  'print(json.dumps({"missing": choose(None, fb), "reads": reads, "zero": choose(0, fb)}))',
].join('\n');

const RUST_GOOD = 'pub fn pick(value: Option<i32>) -> i32 { value.unwrap_or(7) }\n';

const RUST_DEFECT =
  'pub fn pick(value: Option<i32>) -> i32 { match value { Some(v) if v != 0 => v, _ => 7 } }\n';

const RUST_REFACTOR =
  'pub fn pick(value: Option<i32>) -> i32 { if let Some(v) = value { v } else { 7 } }\n';

const RUST_TEST =
  '#[path = "pick.rs"] mod pick; fn main() { println!("{}", pick::pick(Some(0))); }\n';

function buildPatchCheck(): Readonly<Record<string, unknown>> {
  return {
    defect: [],
    expected: { missing: 30, reads: 1, zero: 0 },
    good: [{ find: 'if (!value) {', replace: 'if (value === undefined) {' }],
    id: 'keeps-explicit-zero',
    promise: 'Explicit zero timeout skips fallback read.',
    refactor: [
      {
        find: '  if (!value) {\n    return readDefault();\n  }\n  return value;',
        replace: '  return value === undefined ? readDefault() : value;',
      },
    ],
    runtime: 'node',
    subject: 'large.ts',
    test: LARGE_TEST,
  };
}

function buildPythonCheck(): Check {
  return {
    defect: PYTHON_SRC_BUGGY,
    expected: { missing: 30, reads: 1, zero: 0 },
    good: PYTHON_SRC_GOOD,
    id: 'python-src-layout',
    promise: 'Python src/ package preserves explicit zero.',
    refactor: PYTHON_SRC_REFACTOR,
    runtime: 'python',
    subject: 'src/samplepkg/calc.py',
    test: PYTHON_PROBE,
  };
}

function buildRustCheck(): Check {
  return {
    defect: RUST_DEFECT,
    expected: 0,
    good: RUST_GOOD,
    id: 'rust-explicit-zero',
    promise: 'Rust Option::Some(0) preserves zero.',
    refactor: RUST_REFACTOR,
    runtime: 'rust',
    subject: 'pick.rs',
    test: RUST_TEST,
  };
}

type ReplayInput = Readonly<{
  check: Check;
  files: ReadonlyMap<string, string>;
}>;

async function verifyPatchReplay({ check, files }: ReplayInput): Promise<void> {
  const goodSource = check.good ?? '';
  const byteSize = Buffer.byteLength(goodSource);
  const hasFix = goodSource.includes('value === undefined');

  assert.ok(byteSize > LARGE_MIN_BYTES);
  assert.ok(hasFix);

  const buggyResult = await observe({ check, files, qualify: true });

  assert.equal(buggyResult.passed, false);

  const fixedFiles = new Map<string, string>([['large.ts', goodSource]]);
  const fixedResult = await observe({ check, files: fixedFiles, qualify: true });

  assert.equal(fixedResult.passed, true);
}

void test('patch controls materialize large files and survive replay after source edits', async (context: TestContext) => {
  const files = new Map<string, string>([['large.ts', LARGE_SOURCE]]);
  const candidate = buildPatchCheck();
  const check = validate({ check: candidate, files });

  if (!(await canRun())) {
    context.skip('Isolated sandbox is unavailable');

    return;
  }

  await verifyPatchReplay({ check, files });
});

void test('patch controls reject missing or ambiguous find targets', () => {
  const files = new Map<string, string>([['dup.ts', 'const a = 1;\nconst b = 1;\n']]);

  const ambiguous = {
    defect: [{ find: '= 1;', replace: '= 2;' }],
    expected: 1,
    good: 'const a = 1;\nconst b = 2;\n',
    id: 'ambiguous-edit',
    promise: 'Reject ambiguous edit.',
    refactor: 'const a = 1;\nconst b = 3;\n',
    runtime: 'node',
    subject: 'dup.ts',
    test: 'console.log(1);',
  };

  assert.throws(() => validate({ check: ambiguous, files }), /matched multiple locations/v);

  const missing = { ...ambiguous, defect: [{ find: 'not present', replace: 'x' }] };

  assert.throws(() => validate({ check: missing, files }), /was not found/v);
});

async function verifyPythonSandbox(): Promise<void> {
  const pythonFiles = new Map<string, string>([
    ['src/samplepkg/__init__.py', PYTHON_SRC_INIT],
    ['src/samplepkg/calc.py', PYTHON_SRC_BUGGY],
  ]);

  const pythonCheck = buildPythonCheck();
  const pythonObserved = await observe({ check: pythonCheck, files: pythonFiles, qualify: true });

  assert.equal(pythonObserved.passed, false);

  const fixedPythonFiles = new Map<string, string>([
    ['src/samplepkg/__init__.py', PYTHON_SRC_INIT],
    ['src/samplepkg/calc.py', PYTHON_SRC_GOOD],
  ]);

  const pythonFixed = await observe({
    check: pythonCheck,
    files: fixedPythonFiles,
    qualify: false,
  });

  assert.equal(pythonFixed.passed, true);
}

void test('Python src/ layout and Rust probes execute inside the isolated sandbox', async (context: TestContext) => {
  if (!(await canRun())) {
    context.skip('Isolated sandbox is unavailable');

    return;
  }

  await verifyPythonSandbox();

  const rustFiles = new Map<string, string>([['pick.rs', RUST_DEFECT]]);
  const rustCheck = buildRustCheck();
  const rustObserved = await observe({ check: rustCheck, files: rustFiles, qualify: true });

  assert.equal(rustObserved.passed, false);
});

void test('failed probes surface sanitized stderr diagnostics', async (context: TestContext) => {
  if (!(await canRun())) {
    context.skip('Isolated sandbox is unavailable');

    return;
  }

  const files = new Map<string, string>([['broken.py', 'VALUE = 1\n']]);

  const broken: Check = {
    defect: 'VALUE = 2\n',
    expected: 1,
    good: 'VALUE = 1\n',
    id: 'broken-import',
    promise: 'Broken import reports cause.',
    refactor: 'VALUE = 1 + 0\n',
    runtime: 'python',
    subject: 'broken.py',
    test: 'import nonexistent_tenet_module_xyz\nprint(1)\n',
  };

  await assert.rejects(
    async () => await observe({ check: broken, files, qualify: true }),
    /No module named/v,
  );
});

async function writeRankedModules(root: string): Promise<void> {
  const srcFolder = join(root, 'src', 'pkg');
  const compatPath = join(srcFolder, '_compat.py');
  const corePath = join(srcFolder, 'core.py');

  await mkdir(srcFolder, { recursive: true });
  await writeFile(compatPath, 'def compat():\n    return 1\n');
  await writeFile(corePath, 'def run():\n    return 2\n');
}

void test('discovery ranks public src modules ahead of private underscore internals', async (context: TestContext) => {
  const root = await createProject(context);

  await writeRankedModules(root);

  const selected = { ...contract, paths: ['src/pkg/_compat.py', 'src/pkg/core.py'] };
  const inventory = await readInventory({ contract: selected, root });
  const keys = [...inventory.files.keys()];

  assert.deepEqual(keys, ['src/pkg/core.py', 'src/pkg/_compat.py']);
});

function buildMixedBundle(): Readonly<Record<string, unknown>> {
  const brokenCandidate = {
    defect: [{ find: 'missing-snippet', replace: 'x' }],
    expected: 1,
    good: [],
    id: 'bad-candidate',
    promise: 'Broken candidate edit.',
    refactor: [{ find: 'missing-snippet', replace: 'y' }],
    runtime: 'node',
    subject: 'large.ts',
    test: 'console.log(1);',
  };

  return { checks: [buildPatchCheck(), brokenCandidate], open: [] };
}

async function verifyPartialQualification(root: string): Promise<void> {
  const largePath = join(root, 'large.ts');

  await writeFile(largePath, LARGE_SOURCE);

  const selected = { ...contract, paths: ['large.ts'] };
  const current = await readInventory({ contract: selected, root });
  const value = buildMixedBundle();
  const result = await qualifyBundle({ contractName: 'options', current, value });

  assert.equal(result.qualified.length, 1);
  assert.equal(result.qualified[0]?.id, 'keeps-explicit-zero');
  assert.equal(result.extraUnresolved.length, 1);
  assert.equal(result.extraUnresolved[0]?.check, 'bad-candidate');
}

void test('partial qualification keeps valid checks and records failed candidates separately', async (context: TestContext) => {
  if (!(await canRun())) {
    context.skip('Isolated sandbox is unavailable');

    return;
  }

  const root = await createProject(context);

  await verifyPartialQualification(root);
});
