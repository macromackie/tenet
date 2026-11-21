import { strict as assert } from 'node:assert';
import { mkdir, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { test, type TestContext } from 'node:test';
import { observe } from '../sandbox/index.mts';
import type { Check } from '../types.d.mts';
import { canRun } from './can-run-sandbox.mts';
import { createProject } from './test-project.mts';

type NodeIsolationInput = Readonly<{
  files: ReadonlyMap<string, string>;
  root: string;
  secretPath: string;
}>;

async function createNodeDepProject(root: string): Promise<Readonly<{ secretPath: string }>> {
  const pkgDir = join(root, 'node_modules', 'tiny-double');
  const pkgManifest = join(pkgDir, 'package.json');
  const pkgEntry = join(pkgDir, 'index.mjs');
  const manifestBody = JSON.stringify({ main: 'index.mjs', name: 'tiny-double', type: 'module' });
  const secretPath = join(root, 'outside-secret.txt');

  await mkdir(pkgDir, { recursive: true });
  await writeFile(pkgManifest, manifestBody);
  await writeFile(pkgEntry, 'export function dbl(n) { return n * 2; }\n');
  await writeFile(secretPath, 'top-secret-host-token');

  return { secretPath };
}

function buildNodeDepCheck(): Check {
  return {
    defect: "import { dbl } from 'tiny-double';\nexport const run = (n) => dbl(n) + 1;\n",
    expected: 6,
    good: "import { dbl } from 'tiny-double';\nexport const run = (n) => dbl(n);\n",
    id: 'uses-node-modules',
    promise: 'run doubles its input using tiny-double.',
    refactor: "import { dbl } from 'tiny-double';\nexport function run(n) { return dbl(n); }\n",
    runtime: 'node',
    subject: 'src/calc.js',
    test: 'import { run } from "./src/calc.js";\nconsole.log(JSON.stringify(run(3)));\n',
  };
}

async function assertNodeModulesIsolated({
  files,
  root,
  secretPath,
}: NodeIsolationInput): Promise<void> {
  const goodCheck = buildNodeDepCheck();
  const ok = await observe({ check: goodCheck, files, projectRoot: root, qualify: true });

  assert.equal(ok.passed, true);
  assert.equal(ok.value, 6);

  const writeAttempt: Check = {
    ...goodCheck,
    test: 'import { writeFileSync } from "node:fs";\nwriteFileSync("./node_modules/tiny-double/pwn.txt", "x");\nconsole.log(JSON.stringify(6));\n',
  };

  await assert.rejects(
    async () => await observe({ check: writeAttempt, files, projectRoot: root, qualify: true }),
    /could not observe "good" variant/v,
  );

  const siblingAttempt: Check = {
    ...goodCheck,
    test: `import { readFileSync } from "node:fs";\nreadFileSync(${JSON.stringify(secretPath)}, "utf8");\nconsole.log(JSON.stringify(6));\n`,
  };

  await assert.rejects(
    async () => await observe({ check: siblingAttempt, files, projectRoot: root, qualify: true }),
    /could not observe "good" variant/v,
  );
}

void test('isolated Node sandbox reads project node_modules read-only while denying writes and sibling reads', async (context: TestContext) => {
  if (!(await canRun())) {
    context.skip('Isolated sandbox is unavailable');

    return;
  }

  const root = await createProject(context);
  const { secretPath } = await createNodeDepProject(root);

  const files = new Map<string, string>([
    ['package.json', '{"type":"module"}\n'],
    ['src/calc.js', "import { dbl } from 'tiny-double';\nexport const run = (n) => dbl(n);\n"],
  ]);

  await assertNodeModulesIsolated({ files, root, secretPath });
});

void test('Node missing dependency error surfaces ERR_MODULE_NOT_FOUND instead of Node version banner', async (context: TestContext) => {
  if (!(await canRun())) {
    context.skip('Isolated sandbox is unavailable');

    return;
  }

  const files = new Map<string, string>([
    ['package.json', '{"type":"module"}\n'],
    ['src/a.js', "import missing from 'definitely-not-installed-pkg';\nexport const x = missing;\n"],
  ]);

  const check: Check = {
    defect: "import missing from 'definitely-not-installed-pkg';\nexport const x = missing + 1;\n",
    expected: 1,
    good: "import missing from 'definitely-not-installed-pkg';\nexport const x = missing;\n",
    id: 'missing-pkg',
    promise: 'Missing package diagnostic is explicit.',
    refactor: "import missing from 'definitely-not-installed-pkg';\nexport const x = Number(missing);\n",
    runtime: 'node',
    subject: 'src/a.js',
    test: 'import { x } from "./src/a.js";\nconsole.log(JSON.stringify(x));\n',
  };

  await assert.rejects(
    async () => await observe({ check, files, qualify: true }),
    /ERR_MODULE_NOT_FOUND.*definitely-not-installed-pkg/v,
  );
});
