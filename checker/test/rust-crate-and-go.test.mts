import { strict as assert } from 'node:assert';
import { writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { test, type TestContext } from 'node:test';
import { Option } from 'effect';
import { observe, readGoVersion, readRustCrate } from '../sandbox/index.mts';
import type { Check } from '../types.d.mts';
import { canRun } from './can-run-sandbox.mts';
import { createProject } from './test-project.mts';

type GoIsolationInput = Readonly<{ files: ReadonlyMap<string, string>; root: string }>;

void test('readRustCrate extracts lib name, edition, and transitive default features from Cargo.toml', async () => {
  const manifest = [
    '[package]',
    'name = "demo-crate"',
    'edition = "2018"',
    '[features]',
    'default = ["parsing", "printing"]',
    'parsing = []',
    'printing = ["quote", "proc-macro2/proc-macro"]',
    'quote = []',
  ].join('\n');

  const files = new Map<string, string>([
    ['Cargo.toml', manifest],
    ['src/lib.rs', 'pub mod ops;\n'],
  ]);

  const parsed = await readRustCrate({ files });
  const found = Option.isSome(parsed);

  assert.equal(found, true);

  if (Option.isSome(parsed)) {
    assert.equal(parsed.value.name, 'demo_crate');
    assert.equal(parsed.value.edition, '2018');
    assert.deepEqual(parsed.value.features, ['parsing', 'printing', 'quote']);
  }
});

void test('readRustCrate falls back to projectRoot Cargo.toml when contracts are scoped under src/', async (context: TestContext) => {
  const root = await createProject(context);
  const manifestPath = join(root, 'Cargo.toml');

  await writeFile(
    manifestPath,
    '[package]\nname = "scoped-crate"\nedition = "2021"\n[features]\ndefault = ["parse"]\nparse = []\n',
  );

  const files = new Map<string, string>([['src/lib.rs', 'pub fn ok() -> bool { true }\n']]);
  const parsed = await readRustCrate({ files, projectRoot: root });
  const found = Option.isSome(parsed);

  assert.equal(found, true);

  if (Option.isSome(parsed)) {
    assert.equal(parsed.value.name, 'scoped_crate');
    assert.deepEqual(parsed.value.features, ['parse']);
  }
});

void test('multi-file Rust crate compiles src/lib.rs with default features and qualifies submodule controls', async (context: TestContext) => {
  if (!(await canRun())) {
    context.skip('Isolated sandbox is unavailable');

    return;
  }

  const files = new Map<string, string>([
    [
      'Cargo.toml',
      '[package]\nname = "mini-calc"\nedition = "2021"\n[features]\ndefault = ["double"]\ndouble = []\n',
    ],
    ['src/lib.rs', 'pub mod math;\npub use math::compute;\n'],
    [
      'src/math.rs',
      '#[cfg(feature = "double")]\npub fn compute(x: i32) -> i32 { x * 2 }\n',
    ],
  ]);

  const check: Check = {
    defect: '#[cfg(feature = "double")]\npub fn compute(x: i32) -> i32 { x + 1 }\n',
    expected: 10,
    good: '#[cfg(feature = "double")]\npub fn compute(x: i32) -> i32 { x * 2 }\n',
    id: 'multi-file-rust-crate',
    promise: 'mini_calc::compute doubles its input when the default double feature is enabled.',
    refactor: '#[cfg(feature = "double")]\npub fn compute(x: i32) -> i32 { x + x }\n',
    runtime: 'rust',
    subject: 'src/math.rs',
    test: 'fn main() { println!("{}", mini_calc::compute(5)); }\n',
  };

  const result = await observe({ check, files, qualify: true });

  assert.equal(result.passed, true);
  assert.equal(result.value, 10);
});

async function assertGoIsolated({ files, root }: GoIsolationInput): Promise<void> {
  const secretPath = join(root, 'outside-secret.txt');

  await writeFile(secretPath, 'top-secret-go-token');

  const check: Check = {
    defect: 'package calc\n\nfunc Double(n int) int { return n + 1 }\n',
    expected: 12,
    good: 'package calc\n\nfunc Double(n int) int { return n * 2 }\n',
    id: 'go-double-exported',
    promise: 'calc.Double doubles its integer argument.',
    refactor: 'package calc\n\nfunc Double(n int) int { return n + n }\n',
    runtime: 'go',
    subject: 'calc/calc.go',
    test: [
      'package main',
      'import (',
      '  "encoding/json"',
      '  "fmt"',
      '  "example.com/mini/calc"',
      ')',
      'func main() {',
      '  raw, _ := json.Marshal(calc.Double(6))',
      '  fmt.Println(string(raw))',
      '}',
    ].join('\n'),
  };

  const result = await observe({ check, files, projectRoot: root, qualify: true });

  assert.equal(result.passed, true);
  assert.equal(result.value, 12);

  const readSecretCheck: Check = {
    ...check,
    test: [
      'package main',
      'import (',
      '  "fmt"',
      '  "os"',
      ')',
      'func main() {',
      `  _, err := os.ReadFile(${JSON.stringify(secretPath)})`,
      '  if err != nil { os.Exit(1) }',
      '  fmt.Println(12)',
      '}',
    ].join('\n'),
  };

  await assert.rejects(
    async () => await observe({ check: readSecretCheck, files, projectRoot: root, qualify: true }),
    /could not observe "good" variant/v,
  );
}

void test('Go runtime executes module probes inside the isolated sandbox and denies host reads', async (context: TestContext) => {
  const goVersion = await readGoVersion();

  if (!(await canRun()) || Option.isNone(goVersion)) {
    context.skip('Isolated sandbox or Go toolchain is unavailable');

    return;
  }

  const root = await createProject(context);

  const files = new Map<string, string>([
    ['go.mod', 'module example.com/mini\n\ngo 1.21\n'],
    ['calc/calc.go', 'package calc\n\nfunc Double(n int) int { return n * 2 }\n'],
  ]);

  await assertGoIsolated({ files, root });
});
