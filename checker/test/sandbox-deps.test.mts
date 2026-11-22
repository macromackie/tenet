import { strict as assert } from 'node:assert';
import { createHash } from 'node:crypto';
import { mkdir, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { env as processEnv } from 'node:process';
import { test, type TestContext } from 'node:test';
import { Option } from 'effect';
import { observe, readGoVersion } from '../sandbox/index.mts';
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

function buildNodeDepCheck() {
  return {
    defect: "import { dbl } from 'tiny-double';\nexport const run = (n) => dbl(n) + 1;\n",
    expected: 6,
    good: "import { dbl } from 'tiny-double';\nexport const run = (n) => dbl(n);\n",
    id: 'uses-node-modules',
    promise: 'run doubles its input using tiny-double.',
    refactor: "import { dbl } from 'tiny-double';\nexport function run(n) { return dbl(n); }\n",
    runtime: 'node' as const,
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

  const writeAttempt = {
    ...goodCheck,
    test: 'import { writeFileSync } from "node:fs";\nwriteFileSync("./node_modules/tiny-double/pwn.txt", "x");\nconsole.log(JSON.stringify(6));\n',
  };

  await assert.rejects(
    async () => await observe({ check: writeAttempt, files, projectRoot: root, qualify: true }),
    /could not observe "good" variant/v,
  );

  const siblingAttempt = {
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
    [
      'src/a.js',
      "import missing from 'definitely-not-installed-pkg';\nexport const x = missing;\n",
    ],
  ]);

  const check = {
    defect: "import missing from 'definitely-not-installed-pkg';\nexport const x = missing + 1;\n",
    expected: 1,
    good: "import missing from 'definitely-not-installed-pkg';\nexport const x = missing;\n",
    id: 'missing-pkg',
    promise: 'Missing package diagnostic is explicit.',
    refactor:
      "import missing from 'definitely-not-installed-pkg';\nexport const x = Number(missing);\n",
    runtime: 'node' as const,
    subject: 'src/a.js',
    test: 'import { x } from "./src/a.js";\nconsole.log(JSON.stringify(x));\n',
  };

  await assert.rejects(
    async () => await observe({ check, files, qualify: true }),
    /ERR_MODULE_NOT_FOUND.*definitely-not-installed-pkg/v,
  );
});

type CachedGoModule = Readonly<{ cacheDir: string; goMod: string; goSum: string }>;

type WriteGoCacheFilesInput = Readonly<{
  depMod: string;
  depSrc: string;
  dlDir: string;
  modDir: string;
}>;

function hashGoDir(entries: readonly (readonly [string, string])[]): string {
  const lines = entries
    .map(([name, body]) => {
      const hex = createHash('sha256').update(body).digest('hex');

      return `${hex}  ${name}\n`;
    })
    .toSorted()
    .join('');

  const digest = createHash('sha256').update(lines).digest('base64');

  return `h1:${digest}`;
}

type GoCacheRunInput = Readonly<{
  cacheDir: string;
  files: ReadonlyMap<string, string>;
  root: string;
}>;

async function writeFilesSequentially(
  entries: readonly (readonly [string, string])[],
): Promise<void> {
  for (const [filePath, content] of entries) {
    // Test fixture setup writes a few tiny files sequentially to stay deterministic.
    // oxlint-disable-next-line no-await-in-loop
    await writeFile(filePath, content);
  }
}

async function writeGoCacheFiles({
  depMod,
  depSrc,
  dlDir,
  modDir,
}: WriteGoCacheFilesInput): Promise<Readonly<{ modHash: string; zipHash: string }>> {
  const zipHash = hashGoDir([
    ['example.com/extdep@v1.0.0/extdep.go', depSrc],
    ['example.com/extdep@v1.0.0/go.mod', depMod],
  ]);

  const modHash = hashGoDir([['go.mod', depMod]]);

  const filesToWrite: readonly (readonly [string, string])[] = [
    [join(modDir, 'go.mod'), depMod],
    [join(modDir, 'extdep.go'), depSrc],
    [join(dlDir, 'v1.0.0.ziphash'), `${zipHash}\n`],
    [join(dlDir, 'v1.0.0.mod'), depMod],
  ];

  await mkdir(modDir, { recursive: true });
  await mkdir(dlDir, { recursive: true });
  await writeFilesSequentially(filesToWrite);

  return { modHash, zipHash };
}

async function createSyntheticGoModCache(baseDir: string): Promise<CachedGoModule> {
  const cacheDir = join(baseDir, 'gomodcache');
  const modDir = join(cacheDir, 'example.com', 'extdep@v1.0.0');
  const dlDir = join(cacheDir, 'cache', 'download', 'example.com', 'extdep', '@v');
  const depMod = 'module example.com/extdep\n\ngo 1.21\n';
  const depSrc = 'package extdep\n\nfunc Triple(n int) int { return n * 3 }\n';
  const { modHash, zipHash } = await writeGoCacheFiles({ depMod, depSrc, dlDir, modDir });
  const goMod = 'module example.com/app\n\ngo 1.21\n\nrequire example.com/extdep v1.0.0\n';
  const goSum = `example.com/extdep v1.0.0 ${zipHash}\nexample.com/extdep v1.0.0/go.mod ${modHash}\n`;

  return { cacheDir, goMod, goSum };
}

function buildGoCacheChecks(cacheDir: string) {
  const head = 'package app\nimport "example.com/extdep"\n';
  const target = join(cacheDir, 'pwn.txt');

  const goodCheck = {
    defect: `${head}func Run(n int) int { return extdep.Triple(n) + 1 }\n`,
    expected: 12,
    good: `${head}func Run(n int) int { return extdep.Triple(n) }\n`,
    id: 'go-mod-cache',
    promise: 'app.Run triples its input via a cached external Go module.',
    refactor: `${head}func Run(n int) int { return extdep.Triple(n) + 0 }\n`,
    runtime: 'go' as const,
    subject: 'app/app.go',
    test: 'package main\nimport ("fmt"; "example.com/app/app")\nfunc main() { fmt.Println(app.Run(4)) }\n',
  };

  const writeCheck = {
    ...goodCheck,
    test: `package main\nimport ("fmt"; "os")\nfunc main() { if err := os.WriteFile(${JSON.stringify(target)}, []byte("x"), 0o600); err != nil { os.Exit(1) }; fmt.Println(12) }\n`,
  };

  return { goodCheck, writeCheck };
}

async function runGoCacheChecks({ cacheDir, files, root }: GoCacheRunInput): Promise<void> {
  const { goodCheck, writeCheck } = buildGoCacheChecks(cacheDir);
  const previousCache = processEnv.GOMODCACHE;
  processEnv.GOMODCACHE = cacheDir;

  try {
    const ok = await observe({ check: goodCheck, files, projectRoot: root, qualify: true });

    assert.equal(ok.passed, true);
    assert.equal(ok.value, 12);

    await assert.rejects(
      async () => await observe({ check: writeCheck, files, projectRoot: root, qualify: true }),
      /could not observe "good" variant/v,
    );
  } finally {
    processEnv.GOMODCACHE = previousCache ?? '';
  }
}

async function assertGoCacheIsolated(context: TestContext): Promise<void> {
  const root = await createProject(context);
  const cacheParent = await createProject(context);
  const { cacheDir, goMod, goSum } = await createSyntheticGoModCache(cacheParent);
  const goModPath = join(root, 'go.mod');
  const goSumPath = join(root, 'go.sum');

  await writeFile(goModPath, goMod);
  await writeFile(goSumPath, goSum);

  const files = new Map<string, string>([
    ['go.mod', goMod],
    [
      'app/app.go',
      'package app\nimport "example.com/extdep"\nfunc Run(n int) int { return extdep.Triple(n) }\n',
    ],
  ]);

  await runGoCacheChecks({ cacheDir, files, root });
}

void test('Go sandbox mounts GOMODCACHE read-only and stages go.sum for external modules', async (context: TestContext) => {
  const hasSandbox = await canRun();

  if (!hasSandbox) {
    context.skip('Isolated sandbox is unavailable');

    return;
  }

  const goVersion = await readGoVersion();

  if (Option.isNone(goVersion)) {
    context.skip('Go toolchain is unavailable');

    return;
  }

  await assertGoCacheIsolated(context);
});
