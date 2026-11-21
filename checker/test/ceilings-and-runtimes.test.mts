import { strict as assert } from 'node:assert';
import { mkdir, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { test, type TestContext } from 'node:test';
import { Option } from 'effect';
import { readInventory } from '../discovery/index.mts';
import { type CandidateEvaluation, mergeEvaluations } from '../review/index.mts';
import { observe, readRustCrate, validate } from '../sandbox/index.mts';
import { canRun } from './can-run-sandbox.mts';
import { contract, createProject } from './test-project.mts';

const EXTENSIONLESS_MAIN = [
  'import { FACTOR } from "./constants";',
  'import { addOne } from "./math";',
  'export function compute(value) { return addOne(value) * FACTOR; }',
  '',
].join('\n');

const EXTENSIONLESS_CONSTANTS = 'export const FACTOR = 3;\n';
const EXTENSIONLESS_INDEX = 'export function addOne(value) { return value + 1; }\n';

const RUST_FEATURE_MANIFEST = [
  '[package]',
  'name = "demo_features"',
  'version = "0.1.0"',
  'edition = "2021"',
  '',
  '[features]',
  'default = ["core"]',
  'core = []',
  'full = []',
  'printing = ["quote"]',
  'serde = ["dep:serde"]',
  'test = ["missing_harness/all"]',
  'no-std = []',
  '',
].join('\n');

const RUST_FEATURE_LIB = [
  '#[cfg(feature = "full")]',
  'pub fn full_answer() -> i32 {',
  '    42',
  '}',
  '',
].join('\n');

const RUST_FEATURE_PROBE = [
  'fn main() {',
  '    println!("{}", demo_features::full_answer());',
  '}',
  '',
].join('\n');

function buildSelfPackageRaw(): Readonly<Record<string, unknown>> {
  return {
    defect: [{ find: 'addOne(value) * FACTOR', replace: 'addOne(value)' }],
    expected: 15,
    good: [],
    id: 'self-package-esm-import',
    promise: 'Package self-name and subpath ESM imports resolve to staged source files.',
    refactor: [
      {
        find: 'return addOne(value) * FACTOR;',
        replace: 'const next = addOne(value); return next * FACTOR;',
      },
    ],
    runtime: 'node',
    subject: 'src/index.js',
    test: [
      'import { compute } from "demo-pkg";',
      'import { FACTOR } from "demo-pkg/constants";',
      'console.log(JSON.stringify(compute(3) + FACTOR));',
    ].join('\n'),
  };
}

function buildExtensionlessRaw(): Readonly<Record<string, unknown>> {
  return {
    defect: [{ find: 'addOne(value) * FACTOR', replace: 'addOne(value)' }],
    expected: 12,
    good: [],
    id: 'extensionless-esm-import',
    promise: 'Relative extensionless ESM imports resolve inside the sandbox.',
    refactor: [
      {
        find: 'return addOne(value) * FACTOR;',
        replace: 'const n = addOne(value); return n * FACTOR;',
      },
    ],
    runtime: 'node',
    subject: 'src/main.js',
    test: 'import { compute } from "./src/main.js"; console.log(JSON.stringify(compute(3)));',
  };
}

function buildRustFeatureRaw(): Readonly<Record<string, unknown>> {
  return {
    defect: [{ find: '42', replace: '41' }],
    expected: 42,
    good: [],
    id: 'rust-optional-full-feature',
    promise: 'Local optional Cargo features compile automatically in the sandbox.',
    refactor: [{ find: '42', replace: '21 * 2' }],
    runtime: 'rust',
    subject: 'src/lib.rs',
    test: RUST_FEATURE_PROBE,
  };
}

function buildSampleCheck(id: string) {
  return {
    defect: 'export const x = 0;\n',
    expected: 12,
    good: 'export const x = 1;\n',
    id,
    promise: 'Relative extensionless ESM imports resolve inside the sandbox.',
    refactor: 'export const x = 2 - 1;\n',
    runtime: 'node' as const,
    subject: 'src/main.js',
    test: 'console.log(12);',
  };
}

type WriteNestedInput = Readonly<{
  body: string;
  root: string;
  segments: readonly string[];
}>;

async function writeNestedFile({ body, root, segments }: WriteNestedInput): Promise<void> {
  const fullPath = join(root, ...segments);
  const parentSegments = segments.slice(0, -1);
  const folder = join(root, ...parentSegments);

  await mkdir(folder, { recursive: true });
  await writeFile(fullPath, body);
}

async function seedMixedProject(root: string): Promise<void> {
  await writeNestedFile({ body: 'def run():\n    return 1\n', root, segments: ['src', 'app.py'] });
  await writeNestedFile({
    body: 'def test_run():\n    assert True\n',
    root,
    segments: ['tests', 'test_app.py'],
  });
  await writeNestedFile({ body: 'VALUE = 1\n', root, segments: ['testdata', 'sample.py'] });
}

type SandboxPassInput = Readonly<{
  context: TestContext;
  files: ReadonlyMap<string, string>;
  rawCheck: Readonly<Record<string, unknown>>;
}>;

async function assertSandboxPasses({ context, files, rawCheck }: SandboxPassInput): Promise<void> {
  if (!(await canRun())) {
    context.skip('Isolated sandbox is unavailable');

    return;
  }

  const check = validate({ check: rawCheck, files });
  const result = await observe({ check, files, qualify: true });

  assert.equal(result.passed, true);
}

void test('node sandbox resolves relative extensionless ESM imports and index modules', async (context: TestContext) => {
  const files = new Map<string, string>([
    ['package.json', '{"type":"module"}\n'],
    ['src/constants.js', EXTENSIONLESS_CONSTANTS],
    ['src/main.js', EXTENSIONLESS_MAIN],
    ['src/math/index.js', EXTENSIONLESS_INDEX],
  ]);

  const rawCheck = buildExtensionlessRaw();

  await assertSandboxPasses({ context, files, rawCheck });
});

void test('node sandbox resolves package self-name and subpath ESM imports to staged source', async (context: TestContext) => {
  const files = new Map<string, string>([
    ['package.json', '{"name":"demo-pkg","main":"dist/demo.min.js","type":"module"}\n'],
    ['src/constants.js', EXTENSIONLESS_CONSTANTS],
    ['src/index.js', EXTENSIONLESS_MAIN],
    ['src/math/index.js', EXTENSIONLESS_INDEX],
  ]);

  const rawCheck = buildSelfPackageRaw();

  await assertSandboxPasses({ context, files, rawCheck });
});

void test('readRustCrate enables local optional features and skips missing external deps', async () => {
  const files = new Map<string, string>([
    ['Cargo.toml', RUST_FEATURE_MANIFEST],
    ['src/lib.rs', RUST_FEATURE_LIB],
  ]);

  const crate = await readRustCrate({
    deps: { rustExterns: [{ file: 'libquote-0123456789abcdef.rlib', name: 'quote' }] },
    files,
  });

  const found = Option.isSome(crate);

  assert.ok(found);
  assert.deepEqual(crate.value.features, ['core', 'full', 'printing', 'quote']);
});

void test('rust sandbox enables local optional Cargo features automatically', async (context: TestContext) => {
  const files = new Map<string, string>([
    ['Cargo.toml', RUST_FEATURE_MANIFEST],
    ['src/lib.rs', RUST_FEATURE_LIB],
  ]);

  const rawCheck = buildRustFeatureRaw();

  await assertSandboxPasses({ context, files, rawCheck });
});

void test('discovery omits tests and testdata from prompt while keeping them for execution', async (context: TestContext) => {
  const root = await createProject(context);

  await seedMixedProject(root);

  const selected = {
    ...contract,
    paths: ['src/app.py', 'tests/test_app.py', 'testdata/sample.py'],
  };

  const inventory = await readInventory({ contract: selected, root });
  const promptKeys = [...inventory.files.keys()];
  const runtimeKeys = [...inventory.runtimeFiles.keys()].toSorted();

  assert.deepEqual(promptKeys, ['src/app.py']);
  assert.equal(inventory.omitted.length, 0);
  assert.deepEqual(runtimeKeys, ['src/app.py', 'testdata/sample.py', 'tests/test_app.py']);
});

void test('mergeEvaluations preserves qualified checks and drops rescued unresolved findings', () => {
  const firstCheck = buildSampleCheck('extensionless-esm-import');
  const secondCheck = buildSampleCheck('second-check');

  const prior: CandidateEvaluation = {
    errors: ['second-check: control failed'],
    extraUnresolved: [
      {
        check: 'second-check',
        contract: 'demo',
        cost_usd: null,
        message: 'control failed',
        outcome: 'unresolved',
        requested: false,
        reused: false,
        subject: null,
      },
    ],
    open: ['prior note'],
    qualified: [firstCheck],
  };

  const repaired: CandidateEvaluation = {
    errors: [],
    extraUnresolved: [],
    open: ['repaired note'],
    qualified: [secondCheck],
  };

  const merged = mergeEvaluations({ prior, repaired });
  const mergedIds = merged.qualified.map((check) => check.id);

  assert.deepEqual(mergedIds, ['extensionless-esm-import', 'second-check']);
  assert.equal(merged.extraUnresolved.length, 0);
  assert.deepEqual(merged.open, ['prior note', 'repaired note']);
});
