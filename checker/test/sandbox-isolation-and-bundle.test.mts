import { strict as assert } from 'node:assert';
import { writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { test, type TestContext } from 'node:test';
import { readInventory } from '../discovery/index.mts';
import { qualifyBundle } from '../review/index.mts';
import { observe } from '../sandbox/index.mts';
import type { Check } from '../types.d.mts';
import { canRun } from './can-run-sandbox.mts';
import { contract, createProject } from './test-project.mts';

function buildEscapeCheck(testCode: string): Check {
  return {
    defect: 'export const value = 2;\n',
    expected: 'escaped',
    good: 'export const value = 1;\n',
    id: 'sandbox-escape',
    promise: 'Sandbox denies outside reads and source writes.',
    refactor: 'export const value = 1 + 0;\n',
    runtime: 'node',
    subject: 'sample.ts',
    test: testCode,
  };
}

async function assertWriteDenied(files: ReadonlyMap<string, string>): Promise<void> {
  const writeProbe =
    "import { writeFileSync } from 'node:fs';\nwriteFileSync('./sample.ts', 'mutated');\nconsole.log(JSON.stringify('escaped'));\n";

  const check = buildEscapeCheck(writeProbe);

  await assert.rejects(
    async () => await observe({ check, files, qualify: true }),
    /could not observe "good" variant/v,
  );
}

type OutsideReadInput = Readonly<{
  files: ReadonlyMap<string, string>;
  secretPath: string;
}>;

async function assertOutsideReadDenied({ files, secretPath }: OutsideReadInput): Promise<void> {
  const quoted = JSON.stringify(secretPath);
  const readProbe = `import { readFileSync } from 'node:fs';\nconsole.log(JSON.stringify(readFileSync(${quoted}, 'utf8')));\n`;
  const check = buildEscapeCheck(readProbe);

  await assert.rejects(
    async () => await observe({ check, files, qualify: true }),
    /could not observe "good" variant/v,
  );
}

void test('isolated sandbox blocks writes to staged source and reads outside work', async (context: TestContext) => {
  if (!(await canRun())) {
    context.skip('Isolated sandbox is unavailable');

    return;
  }

  const outsideRoot = await createProject(context);
  const secretPath = join(outsideRoot, 'outside-secret.txt');

  await writeFile(secretPath, 'host-only-data');

  const files = new Map<string, string>([['sample.ts', 'export const value = 1;\n']]);

  await assertWriteDenied(files);
  await assertOutsideReadDenied({ files, secretPath });
});

void test('rust compiler diagnostics surface the error code ahead of abort footer', async (context: TestContext) => {
  if (!(await canRun())) {
    context.skip('Isolated sandbox is unavailable');

    return;
  }

  const files = new Map<string, string>([['lib.rs', 'pub fn f() -> i32 { 1 }\n']]);

  const brokenRust: Check = {
    defect: 'pub fn f() -> i32 { 2 }\n',
    expected: 1,
    good: 'pub fn f() -> i32 { 1 }\n',
    id: 'rust-compile-error',
    promise: 'Rust compile errors show error[E0433] instead of abort footer.',
    refactor: 'pub fn f() -> i32 { 0 + 1 }\n',
    runtime: 'rust',
    subject: 'lib.rs',
    test: 'fn main() { println!("{}", undeclared_mod::f()); }\n',
  };

  await assert.rejects(
    async () => await observe({ check: brokenRust, files, qualify: true }),
    /error\[E0433\]/v,
  );
});

type EmptyBundleInput = Readonly<{
  current: Awaited<ReturnType<typeof readInventory>>;
  value: unknown;
}>;

async function assertEmptyRejection({ current, value }: EmptyBundleInput): Promise<void> {
  const result = await qualifyBundle({ contractName: 'options', current, value });

  assert.equal(result.qualified.length, 0);
  assert.equal(result.extraUnresolved.length, 1);
  assert.equal(result.extraUnresolved[0]?.outcome, 'unresolved');
}

async function assertMalformedBundle(root: string): Promise<void> {
  const file = join(root, 'value.ts');

  await writeFile(file, 'export const value = 1;\n');

  const selected = { ...contract, paths: ['value.ts'] };
  const current = await readInventory({ contract: selected, root });

  await assertEmptyRejection({ current, value: { checks: [], open: [] } });
  await assertEmptyRejection({ current, value: { checks: [], open: ['   '] } });
}

void test('malformed top-level bundles produce explicit unresolved findings', async (context: TestContext) => {
  const root = await createProject(context);

  await assertMalformedBundle(root);
});
