import { strict as assert } from 'node:assert';
import { mkdir, utimes, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { env } from 'node:process';
import { test, type TestContext } from 'node:test';
import { Option } from 'effect';
import { runAuditFix } from '../audit/index.mts';
import { buildGoCommand, observe, readGoVersion, resolveProbeFile } from '../sandbox/index.mts';
import { canRun } from './can-run-sandbox.mts';
import { createProject } from './test-project.mts';

type ObserveCheck = Parameters<typeof observe>[0]['check'];
type FixResponse = Awaited<ReturnType<typeof runAuditFix>>;
type SalvageAssertInput = Readonly<{ capturedBodies: readonly string[]; response: FixResponse }>;
type BuildDirInput = Readonly<{ dir: string; generated: string; mtimeMs: number; output: string }>;

const RETURNED_COST = 0.001;
const REPO_MATH_TEST = 'import assert from "node:assert";\n// sentinel: expect addOne(2) === 3\n';
const BUGGY_MATH = 'export function addOne(n) { return n + 2; }\nexport function double(n) { return n + 1; }\nexport function keepZero(n) { return n === 0 ? 0 : n; }';

const FIX_RESPONSES: readonly Readonly<Record<string, unknown>>[] = [
  {
    edits: [{ find: 'export function addOne(n) { return n + 2; }', line: 1, path: 'src/math.mjs', replace: 'export function addOne(n) { return n + 1; }' }],
    summary: 'Fix addOne while preserving keepZero.',
  },
  {
    edits: [{ find: BUGGY_MATH, line: 1, path: 'src/math.mjs', replace: 'export function addOne(n) { return n + 1; }\nexport function double(n) { return n * 2; }\nexport function keepZero(n) { return n === 0 ? 1 : n; }' }],
    summary: 'Fixes both witnesses but regresses keepZero guard.',
  },
  {
    edits: [{ find: 'nonexistent line', path: 'src/math.mjs', replace: 'broken' }],
    summary: 'Unmatched edit on final attempt.',
  },
];

const MATH_GUARDS = [
  {
    check: 'keep-zero',
    contract: 'math',
    repro: { code: 'import { keepZero } from "./src/math.mjs"; console.log(JSON.stringify(keepZero(0)));', expected: '0', runtime: 'node' },
    subject: 'src/math.mjs',
  },
];

const MATH_WITNESSES = [
  {
    check: 'add-one',
    contract: 'math',
    message: 'expected 3, observed 4',
    repro: { code: 'import { addOne } from "./src/math.mjs"; console.log(JSON.stringify(addOne(2)));', expected: '3', runtime: 'node' },
    subject: 'src/math.mjs',
  },
  {
    check: 'double-val',
    contract: 'math',
    message: 'expected 6, observed 4',
    repro: { code: 'import { double } from "./src/math.mjs"; console.log(JSON.stringify(double(3)));', expected: '6', runtime: 'node' },
    subject: 'src/math.mjs',
  },
];

function buildFixStream(payload: Readonly<Record<string, unknown>>): string {
  const content = JSON.stringify(payload);
  const delta = JSON.stringify({ choices: [{ delta: { content }, index: 0 }] });
  const finish = JSON.stringify({ choices: [{ delta: {}, finish_reason: 'stop', index: 0 }], usage: { cost: RETURNED_COST } });

  return `data: ${delta}\n\ndata: ${finish}\n\ndata: [DONE]\n\n`;
}

void test('node-resolve-hook elides value-style type imports and re-exports inside the OS sandbox', async (context: TestContext) => {
  if (!(await canRun())) {
    context.skip('Isolated sandbox is unavailable');

    return;
  }

  const typesSrc = 'export interface QueryValue { key: string; val: string }\nexport type QueryMap = Record<string, QueryValue>;\nexport function parseQuery(raw: string): QueryMap { const [key = "", val = ""] = raw.split("="); return { [key]: { key, val } }; }\n';
  const consumerGood = 'import { QueryValue, QueryMap, parseQuery } from "./barrel";\nimport { QueryValue as QV } from "./types";\nexport { QueryValue, parseQuery } from "./barrel";\ninterface LocalMeta { count: number }\nexport { LocalMeta };\nexport function readValue(raw: string): string { const parsed: QueryMap = parseQuery(raw); const item: QV | undefined = parsed.a; return item ? item.val : ""; }\n';

  const files = new Map<string, string>([
    ['src/types.ts', typesSrc],
    ['src/barrel.ts', 'export * from "./types";\n'],
    ['src/consumer.ts', consumerGood],
  ]);

  const check: ObserveCheck = {
    defect: consumerGood.replace('item ? item.val : ""', '""'),
    expected: 'ok',
    good: consumerGood,
    id: 'ts-type-import-elision',
    promise: 'Value-style TypeScript type imports and re-exports elide cleanly under Node.',
    refactor: consumerGood.replace('item ? item.val : ""', 'item?.val ?? ""'),
    runtime: 'node',
    subject: 'src/consumer.ts',
    test: 'import { readValue } from "./src/consumer"; console.log(JSON.stringify(readValue("a=ok")));',
  };

  const observation = await observe({ check, files, qualify: true });

  assert.deepEqual([observation.passed, observation.value], [true, 'ok']);
});

async function writeBuildScriptDir({ dir, generated, mtimeMs, output }: BuildDirInput): Promise<void> {
  const outDir = join(dir, 'out');
  const outputPath = join(dir, 'output');
  const genPath = join(outDir, 'generated.rs');
  const stamp = new Date(mtimeMs);

  await mkdir(outDir, { recursive: true });
  await writeFile(outputPath, output);
  await writeFile(genPath, generated);
  await utimes(dir, stamp, stamp);
}

async function writeRustBuildFixture(root: string): Promise<void> {
  const manifestPath = join(root, 'Cargo.toml');
  const olderDir = join(root, 'target', 'debug', 'build', 'cfg-demo-0000000000000001');
  const newerDir = join(root, 'target', 'debug', 'build', 'cfg-demo-0123456789abcdef');
  const binaryOnlyDir = join(root, 'target', 'debug', 'build', 'cfg-demo-ffffffffffffffff');
  const binaryPath = join(binaryOnlyDir, 'build-script-build');

  await writeFile(manifestPath, '[package]\nname = "cfg-demo"\nedition = "2021"\n');
  await mkdir(binaryOnlyDir, { recursive: true });
  await writeFile(binaryPath, 'placeholder');
  await writeBuildScriptDir({ dir: olderDir, generated: 'pub const GENERATED_BONUS: i32 = 1;\n', mtimeMs: 1_700_000_000_000, output: 'cargo:rustc-cfg=stale_flag\n' });
  await writeBuildScriptDir({ dir: newerDir, generated: 'pub const GENERATED_BONUS: i32 = 40;\n', mtimeMs: 1_700_000_100_000, output: 'cargo:rustc-cfg=has_prebuilt_flag\ncargo::rustc-cfg=mode="fast"\ncargo:rustc-cfg=unsafe;injected\n' });
}

void test('Rust sandbox applies prebuilt build.rs rustc-cfg flags and read-only OUT_DIR include!', async (context: TestContext) => {
  if (!(await canRun())) {
    context.skip('Isolated sandbox is unavailable');

    return;
  }

  const root = await createProject(context);

  await writeRustBuildFixture(root);

  const libGood = 'include!(concat!(env!("OUT_DIR"), "/generated.rs"));\n#[cfg(all(has_prebuilt_flag, mode = "fast"))]\npub fn answer(x: i32) -> i32 { x + GENERATED_BONUS }\n';

  const files = new Map<string, string>([
    ['Cargo.toml', '[package]\nname = "cfg-demo"\nedition = "2021"\n'],
    ['src/lib.rs', libGood],
  ]);

  const check: ObserveCheck = {
    defect: libGood.replace('x + GENERATED_BONUS', 'x'),
    expected: 42,
    good: libGood,
    id: 'rust-prebuilt-build-rs',
    promise: 'cfg_demo::answer uses prebuilt build.rs rustc-cfg flags and OUT_DIR include!.',
    refactor: libGood.replace('x + GENERATED_BONUS', 'GENERATED_BONUS + x'),
    runtime: 'rust',
    subject: 'src/lib.rs',
    test: 'fn main() { println!("{}", cfg_demo::answer(2)); }\n',
  };

  const observation = await observe({ check, files, projectRoot: root, qualify: true });

  assert.deepEqual([observation.passed, observation.value], [true, 42]);
});

function buildGoMainCheck(): ObserveCheck {
  const mainGood = 'package main\nfunc formatGreeting(name string) string { return "hello " + name }\nfunc main() {}\n';
  const probe = 'package main\nimport ("encoding/json"; "fmt"; "testing")\nfunc TestTenetObserve(t *testing.T) { raw, _ := json.Marshal(formatGreeting("tenet")); fmt.Println("__TENET_JSON__:" + string(raw)) }\n';

  return {
    defect: mainGood.replace('"hello " + name', '"bye " + name'),
    expected: 'hello tenet',
    good: mainGood,
    id: 'go-main-same-package',
    promise: 'cmd/tool formatGreeting prefixes the input with hello.',
    refactor: mainGood.replace('"hello " + name', '"hello" + " " + name'),
    runtime: 'go',
    subject: 'cmd/tool/main.go',
    test: probe,
  };
}

function assertGoProbeCommand(check: ObserveCheck): void {
  const emptyFiles = new Map<string, string>();
  const subProbe = resolveProbeFile(check);
  const rootProbe = resolveProbeFile({ ...check, subject: 'main.go' });
  const cmd = buildGoCommand({ binary: '/usr/local/go/bin/go', deps: { rustBuildCfgs: [], rustExterns: [] }, files: emptyFiles, isTestProbe: true, output: '/tmp/observe', probeDir: 'cmd/tool' });
  const pkgArg = cmd.at(5);

  assert.deepEqual([subProbe, rootProbe, pkgArg], ['cmd/tool/tenet_observe_test.go', 'tenet_observe_test.go', './cmd/tool']);
}

void test('Go same-package TestTenetObserve stages alongside package main and runs go test', async (context: TestContext) => {
  const check = buildGoMainCheck();

  assertGoProbeCommand(check);

  const hasSandbox = await canRun();
  const goVersion = await readGoVersion();

  if (!hasSandbox || Option.isNone(goVersion)) {
    return;
  }

  const root = await createProject(context);

  const files = new Map<string, string>([
    ['go.mod', 'module example.com/cli\n\ngo 1.21\n'],
    ['cmd/tool/main.go', check.good],
  ]);

  const observation = await observe({ check, files, projectRoot: root, qualify: true });

  assert.deepEqual([observation.passed, observation.value], [true, 'hello tenet']);
});

type FixtureFileInput = Readonly<{ body: string; folder: string; name: string; root: string }>;

async function writeFixtureFile({ body, folder, name, root }: FixtureFileInput): Promise<void> {
  const dir = join(root, folder);
  const filePath = join(dir, name);

  await mkdir(dir, { recursive: true });
  await writeFile(filePath, body);
}

async function writeFixFixture(root: string): Promise<string> {
  const state = join(root, '.tenet-state');

  await mkdir(state, { recursive: true });
  await writeFixtureFile({ body: `${BUGGY_MATH}\n`, folder: 'src', name: 'math.mjs', root });
  await writeFixtureFile({ body: REPO_MATH_TEST, folder: 'test', name: 'math.test.mjs', root });

  return state;
}

function installFixFetchMock(context: TestContext): readonly string[] {
  const capturedBodies: string[] = [];
  let attemptIndex = 0;

  // Mock fetch receives (url, init) matching globalThis.fetch.
  // oxlint-disable-next-line max-params
  context.mock.method(globalThis, 'fetch', (_url: unknown, init?: RequestInit): Response => {
    if (typeof init?.body === 'string') {
      capturedBodies.push(init.body);
    }

    const reply = FIX_RESPONSES.at(attemptIndex) ?? FIX_RESPONSES[2];
    const stream = buildFixStream(reply);

    attemptIndex += 1;

    return new Response(stream, { headers: { 'content-type': 'text/event-stream' } });
  });

  return capturedBodies;
}

function assertSalvageOutcome({ capturedBodies, response }: SalvageAssertInput): void {
  const patch = response.patches.at(0);
  const statuses = response.outcomes.map((item) => [item.check, item.status]);
  const firstBody = capturedBodies.at(0) ?? '';
  const hasTestSentinel = firstBody.includes('sentinel: expect addOne(2) === 3');

  assert.equal(hasTestSentinel, true);
  assert.deepEqual([response.requests, response.patches.length, patch?.path], [3, 1, 'src/math.mjs']);
  assert.ok(patch !== undefined);

  const hasAddOneFix = patch.content.includes('export function addOne(n) { return n + 1; }');
  const keepsZero = patch.content.includes('export function keepZero(n) { return n === 0 ? 0 : n; }');

  assert.deepEqual([hasAddOneFix, keepsZero, statuses], [true, true, [['add-one', 'fixed'], ['double-val', 'failed']]]);
}

async function runSalvageScenario(context: TestContext): Promise<void> {
  const root = await createProject(context);
  const state = await writeFixFixture(root);
  const capturedBodies = installFixFetchMock(context);
  const previousKey = env.OPENROUTER_API_KEY;

  env.OPENROUTER_API_KEY = 'fixture';

  try {
    const group = { contracts: [], files: ['src/math.mjs'], guards: MATH_GUARDS, name: 'math', testFiles: ['test/math.test.mjs'], witnesses: MATH_WITNESSES };
    const response = await runAuditFix({ groups: [group], maxRequests: 3, mode: 'audit-fix', root, runtimePaths: ['src/math.mjs'], state });

    assertSalvageOutcome({ capturedBodies, response });
  } finally {
    env.OPENROUTER_API_KEY = previousKey ?? '';
  }
}

void test('runAuditFix supplies bounded testFiles to prompts and salvages the best zero-regression partial fix', async (context: TestContext) => {
  if (!(await canRun())) {
    context.skip('Isolated sandbox is unavailable');

    return;
  }

  await runSalvageScenario(context);
});
