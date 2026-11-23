import { strict as assert } from 'node:assert';
import { mkdir, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { test, type TestContext } from 'node:test';
import { Option } from 'effect';
import { isHiddenDirectoryPath, readInventory } from '../discovery/index.mts';
import {
  applyControl,
  detectMissingExternalPackage,
  formatProbeFailure,
  isMissingExternalDepMessage,
  parseProbeStdout,
  validate,
} from '../sandbox/index.mts';
import { contract, createProject } from './test-project.mts';

const TEST_LIMIT_BYTES = 131_072;
const LARGE_PAD_REPEATS = 4_500;
const HUGE_MIN_BYTES = 140_000;
const FILLER_LINES = 20;
const LINE_HINT_TARGET = 25;
const EXPECTED_COUNT = 3;
const EXPECTED_PASSED = 12;

void test('parseProbeStdout parses pure JSON, noisy logs, sentinels, and trailing multi-line blocks', () => {
  const pure = parseProbeStdout('{"ok":true,"count":3}\n');
  const noisy = parseProbeStdout('[GIN-debug] GET /ping --> main.handler\n{"ok":true}\n');

  const sentinelRaw =
    'debug line\n__TENET_JSON__:{"first":1}\nmore logs\n__TENET_JSON__:{"second":2}\ntrailing noise\n';

  const sentinel = parseProbeStdout(sentinelRaw);
  const multiline = parseProbeStdout('[INFO] boot\n{\n  "ok": false,\n  "passed": 12\n}\n');

  assert.deepEqual(pure, { count: EXPECTED_COUNT, ok: true });
  assert.deepEqual(noisy, { ok: true });
  assert.deepEqual(sentinel, { second: 2 });
  assert.deepEqual(multiline, { ok: false, passed: EXPECTED_PASSED });
});

void test('parseProbeStdout rejects invalid sentinel or non-JSON stdout with sanitized last line', () => {
  assert.throws(
    () => parseProbeStdout('booting server\nnot valid json output\n'),
    /not valid json output/v,
  );

  assert.throws(
    () => parseProbeStdout('__TENET_JSON__:{broken\n{"ok":true}\n'),
    /__TENET_JSON__:\{broken/v,
  );

  assert.throws(() => parseProbeStdout('   \n  '), /empty stdout/v);
});

void test('applyControl normalizes bidirectional LF/CRLF and re-indents trimmed block matches', () => {
  const crlfSource = 'function demo() {\r\n  return 0;\r\n}\r\n';

  const lfEdit = [
    { find: 'function demo() {\n  return 0;\n}', replace: 'function demo() {\n  return 1;\n}' },
  ];

  const updatedCrlf = applyControl({ control: lfEdit, limit: TEST_LIMIT_BYTES, source: crlfSource });

  assert.equal(updatedCrlf, 'function demo() {\r\n  return 1;\r\n}\r\n');

  const lfSource = 'function demo() {\n    if (flag) {\n        return 0;\n    }\n}\n';

  const crlfEdit = [
    { find: 'if (flag) {\r\n    return 0;\r\n}', replace: 'if (flag) {\r\n    return 1;\r\n}' },
  ];

  const updatedIndented = applyControl({
    control: crlfEdit,
    limit: TEST_LIMIT_BYTES,
    source: lfSource,
  });

  assert.equal(
    updatedIndented,
    'function demo() {\n    if (flag) {\n        return 1;\n    }\n}\n',
  );
});

void test('applyControl disambiguates via line or occurrence and reports match line numbers on ambiguity', () => {
  const filler = '// pad\n'.repeat(FILLER_LINES);
  const source = `const a = 0;\n${filler}const b = 0;\n`;

  const byLine = applyControl({
    control: [{ find: '= 0;', line: LINE_HINT_TARGET, replace: '= 1;' }],
    limit: TEST_LIMIT_BYTES,
    source,
  });

  const byOccurrence = applyControl({
    control: [{ find: '= 0;', occurrence: 1, replace: '= 2;' }],
    limit: TEST_LIMIT_BYTES,
    source,
  });

  const lineResults = [byLine.startsWith('const a = 0;\n'), byLine.endsWith('const b = 1;\n')];

  const occurrenceResults = [
    byOccurrence.startsWith('const a = 2;\n'),
    byOccurrence.endsWith('const b = 0;\n'),
  ];

  assert.deepEqual(lineResults, [true, true]);
  assert.deepEqual(occurrenceResults, [true, true]);

  assert.throws(
    () =>
      applyControl({
        control: [{ find: '= 0;', replace: '= 1;' }],
        limit: TEST_LIMIT_BYTES,
        source,
      }),
    /lines L1, L22/v,
  );
});

void test('validate allows array edits on large source files exceeding the 128 KiB string limit', () => {
  const hugePadding = '// padding line for large source file\n'.repeat(LARGE_PAD_REPEATS);
  const hugeSource = `${hugePadding}export const value = 0;\n`;
  const sourceBytes = Buffer.byteLength(hugeSource);

  assert.ok(sourceBytes > HUGE_MIN_BYTES);

  const files = new Map<string, string>([['huge.ts', hugeSource]]);

  const checked = validate({
    check: {
      defect: [],
      expected: 1,
      good: [{ find: 'export const value = 0;', replace: 'export const value = 1;' }],
      id: 'huge-file-edit',
      promise: 'Huge file edit succeeds.',
      runtime: 'node',
      subject: 'huge.ts',
      test: 'import { value } from "./huge.ts"; console.log(value);',
    },
    files,
  });

  const hasFix = checked.good.includes('export const value = 1;');

  assert.deepEqual([checked.brownfield, hasFix], [true, true]);
});

function assertExternalMissingPackage(): void {
  const chainedTraceback = [
    'Traceback (most recent call last):',
    '  File "a.py", line 1, in <module>',
    'ValueError: first error',
    'During handling of the above exception, another exception occurred:',
    "ModuleNotFoundError: No module named 'matplotlib'",
  ].join('\n');

  const externalMessage = formatProbeFailure({
    base: 'the isolated test exited with status 1',
    filePaths: ['src/mypkg/__init__.py', 'src/mypkg/core.py'],
    runtime: 'python',
    stderr: chainedTraceback,
  });

  const hasError = externalMessage.includes('ModuleNotFoundError');
  const hasHint = externalMessage.includes('missing external package "matplotlib"');
  const isUnrecoverable = isMissingExternalDepMessage(externalMessage);

  assert.deepEqual([hasError, hasHint, isUnrecoverable], [true, true, true]);
}

void test('probe diagnostics detect external missing packages while keeping repo-local import typos repairable', () => {
  assertExternalMissingPackage();

  const localDetected = detectMissingExternalPackage({
    filePaths: ['src/mypkg/__init__.py', 'src/mypkg/core.py'],
    runtime: 'python',
    stderr: "ModuleNotFoundError: No module named 'mypkg.typo_submod'\n",
  });

  const relativeDetected = detectMissingExternalPackage({
    filePaths: ['src/index.mjs'],
    runtime: 'node',
    stderr: "Error [ERR_MODULE_NOT_FOUND]: Cannot find module './missing-local.mjs'\n",
  });

  const missingFlags = [Option.isNone(localDetected), Option.isNone(relativeDetected)];

  assert.deepEqual(missingFlags, [true, true]);
});

async function writeHiddenFiles(root: string): Promise<void> {
  const yarnDir = join(root, '.yarn', 'plugins');
  const venvDir = join(root, '.venv', 'lib', 'pkg');
  const yarnFile = join(yarnDir, 'plugin.cjs');
  const venvPy = join(venvDir, 'dep.py');
  const venvC = join(venvDir, 'native.c');

  await mkdir(yarnDir, { recursive: true });
  await mkdir(venvDir, { recursive: true });
  await writeFile(yarnFile, 'module.exports = {};\n');
  await writeFile(venvPy, 'x = 1\n');
  await writeFile(venvC, 'int x = 1;\n');
}

async function writeHiddenDirFixture(root: string): Promise<void> {
  await writeHiddenFiles(root);

  const srcDir = join(root, 'src');
  const eslintPath = join(root, '.eslintrc.cjs');
  const appPath = join(srcDir, 'app.ts');

  await mkdir(srcDir, { recursive: true });
  await writeFile(eslintPath, 'module.exports = {};\n');
  await writeFile(appPath, 'export const ok = true;\n');
}

function assertHiddenPathClassification(): void {
  const results = [
    isHiddenDirectoryPath('.yarn/plugins/@yarnpkg/plugin.cjs'),
    isHiddenDirectoryPath('.venv/lib/python3.11/site-packages/foo.py'),
    isHiddenDirectoryPath('packages/app/.next/server/page.js'),
    isHiddenDirectoryPath('.eslintrc.cjs'),
    isHiddenDirectoryPath('./src/app.ts'),
    isHiddenDirectoryPath('../src/app.ts'),
  ];

  assert.deepEqual(results, [true, true, true, false, false, false]);
}

void test('isHiddenDirectoryPath and readInventory skip dot-directories with zero omitted noise', async (context: TestContext) => {
  assertHiddenPathClassification();

  const root = await createProject(context);

  await writeHiddenDirFixture(root);

  const scoped = {
    ...contract,
    paths: [
      '.eslintrc.cjs',
      '.venv/lib/pkg/dep.py',
      '.venv/lib/pkg/native.c',
      '.yarn/plugins/plugin.cjs',
      'src/app.ts',
    ],
  };

  const inventory = await readInventory({ contract: scoped, root });
  const promptPaths = [...inventory.files.keys()];

  assert.deepEqual(inventory.omitted, []);
  assert.deepEqual(promptPaths, ['src/app.ts', '.eslintrc.cjs']);
});
