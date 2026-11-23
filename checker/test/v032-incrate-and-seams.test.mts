import { strict as assert } from 'node:assert';
import { mkdir, writeFile } from 'node:fs/promises';
import { env } from 'node:process';
import { test, type TestContext } from 'node:test';
import { Option, Schema } from 'effect';
import { runAudit } from '../audit/index.mts';
import { readInventory } from '../discovery/index.mts';
import { buildRustCommand, isRustInCrateProbe, observe } from '../sandbox/index.mts';
import { canRun } from './can-run-sandbox.mts';
import { contract, createProject } from './test-project.mts';

type ObserveCheck = Parameters<typeof observe>[0]['check'];
type AuditResponse = Awaited<ReturnType<typeof runAudit>>;
type VerifyAssertInput = Readonly<{ captured: readonly string[]; facadeHeader: string; response: AuditResponse }>;

const RETURNED_COST = 0.001;
const INTERNAL_GOOD = 'pub(crate) fn secret_add(x: i32, y: i32) -> i32 { x + y }\n';
const INTERNAL_DEFECT = 'pub(crate) fn secret_add(x: i32, y: i32) -> i32 { x - y }\n';
const INTERNAL_REFACTOR = 'pub(crate) fn secret_add(x: i32, y: i32) -> i32 { y + x }\n';
const CRATE_MANIFEST = '[package]\nname = "seam-demo"\nedition = "2021"\n';
const CRATE_LIB = 'mod internal;\npub fn facade(x: i32) -> i32 { internal::secret_add(x, 1) }\n';
const LIB_GOOD = 'fn private_scale(x: i32) -> i32 { x * 3 }\npub fn pub_api() -> i32 { 0 }\n';
const LIB_DEFECT = 'fn private_scale(x: i32) -> i32 { x * 2 }\npub fn pub_api() -> i32 { 0 }\n';
const LIB_REFACTOR = 'fn private_scale(x: i32) -> i32 { x + x + x }\npub fn pub_api() -> i32 { 0 }\n';
const FACADE_HEADER = 'import { compute } from "./worker.mjs";\nexport function runWorker(x) { return compute(x); }\n';
const IN_CRATE_PROBE = String.raw`#[test] fn tenet_observe() { println!("__TENET_JSON__:{{\"ok\":{}}}", crate::internal::secret_add(20, 22) == 42); }`;
const LIB_SUBJECT_PROBE = String.raw`#[test] fn tenet_observe() { println!("__TENET_JSON__:{{\"ok\":{}}}", super::private_scale(7) == 21); }`;

const RawRecord = Schema.Record(Schema.String, Schema.Unknown);
const JsonRecord = Schema.fromJsonString(RawRecord);
const RawList = Schema.Array(RawRecord);
const SampleMap = Schema.Record(Schema.String, Schema.String);
const decodeJsonRecord = Schema.decodeUnknownOption(JsonRecord, { onExcessProperty: 'error' });
const decodeRawList = Schema.decodeUnknownOption(RawList, { onExcessProperty: 'error' });
const decodeSampleMap = Schema.decodeUnknownOption(SampleMap, { onExcessProperty: 'error' });
const decodeString = Schema.decodeUnknownOption(Schema.String, { onExcessProperty: 'error' });

function buildInCrateCheck(): ObserveCheck {
  return { defect: INTERNAL_DEFECT, expected: { ok: true }, good: INTERNAL_GOOD, id: 'rust-incrate-internal', promise: 'crate::internal::secret_add adds its two arguments.', refactor: INTERNAL_REFACTOR, runtime: 'rust', subject: 'src/internal.rs', test: IN_CRATE_PROBE };
}

function assertInCrateDetection(check: ObserveCheck): void {
  const libFiles = new Map<string, string>([['src/lib.rs', CRATE_LIB]]);
  const binFiles = new Map<string, string>([['src/main.rs', 'fn main() {}\n']]);
  const mainProbe: ObserveCheck = { ...check, test: 'fn main() { println!("true"); }\n' };
  const nodeCheck: ObserveCheck = { ...check, runtime: 'node' };
  const detected = isRustInCrateProbe({ check, files: libFiles });
  const mainDetected = isRustInCrateProbe({ check: mainProbe, files: libFiles });
  const binDetected = isRustInCrateProbe({ check, files: binFiles });
  const nodeDetected = isRustInCrateProbe({ check: nodeCheck, files: libFiles });

  assert.deepEqual([detected, mainDetected, binDetected, nodeDetected], [true, false, false, false]);
}

function assertTestRustCommand(testCmd: readonly string[]): void {
  const script = testCmd.at(2) ?? '';
  const hasTest = script.includes('--test');
  const hasHarness = script.includes('./src/__tenet_lib.rs');
  const hasFilter = script.includes('__tenet_observe_probe::tenet_observe --exact --nocapture');
  const hasFeature = testCmd.includes('feature="std"');
  const hasBuildCfg = testCmd.includes('custom_cfg');
  const hasExtern = testCmd.includes('foo=/deps/rust/libfoo.rlib');

  assert.deepEqual([hasTest, hasHarness, hasFilter, hasFeature, hasBuildCfg, hasExtern], [true, true, true, true, true, true]);
}

function assertRustCommands(): void {
  const rustCrate = Option.some({ edition: '2021', features: ['std'], libPath: 'src/lib.rs', name: 'seam_demo' });
  const deps = { rustBuildCfgs: ['custom_cfg'], rustDepsDir: '/deps/rust', rustExterns: [{ file: 'libfoo.rlib', name: 'foo' }] };
  const base = { deps, depsMount: '/deps/rust', probeFile: '.tenet-observe.rs', probeTmp: '/tmp', rustCrate, rustc: '/toolchain/bin/rustc' };
  const testCmd = buildRustCommand({ ...base, inCrateTest: true, linkerArg: '/usr/bin/gcc' });
  const normalCmd = buildRustCommand({ ...base, inCrateTest: false });
  const normalScript = normalCmd.at(2) ?? '';
  const hasLibType = normalScript.includes('--crate-type=lib');
  const hasLibPath = normalScript.includes('./src/lib.rs');

  assertTestRustCommand(testCmd);
  assert.deepEqual([hasLibType, hasLibPath], [true, true]);
}

async function assertInCrateSandbox(check: ObserveCheck): Promise<void> {
  const goodFiles = new Map<string, string>([['Cargo.toml', CRATE_MANIFEST], ['src/lib.rs', CRATE_LIB], ['src/internal.rs', INTERNAL_GOOD]]);
  const defectFiles = new Map<string, string>([['Cargo.toml', CRATE_MANIFEST], ['src/lib.rs', CRATE_LIB], ['src/internal.rs', INTERNAL_DEFECT]]);
  const goodObs = await observe({ check, files: goodFiles, qualify: true });
  const defectObs = await observe({ check, files: defectFiles, qualify: false });

  assert.deepEqual([goodObs.passed, goodObs.value, defectObs.passed, defectObs.value], [true, { ok: true }, false, { ok: false }]);
}

async function assertLibSubjectInCrate(): Promise<void> {
  const libCheck: ObserveCheck = { defect: LIB_DEFECT, expected: { ok: true }, good: LIB_GOOD, id: 'rust-incrate-lib-subject', promise: 'private_scale triples its input even when subject is src/lib.rs.', refactor: LIB_REFACTOR, runtime: 'rust', subject: 'src/lib.rs', test: LIB_SUBJECT_PROBE };
  const libFiles = new Map<string, string>([['Cargo.toml', CRATE_MANIFEST], ['src/lib.rs', LIB_GOOD]]);
  const libObs = await observe({ check: libCheck, files: libFiles, qualify: true });

  assert.deepEqual([libObs.passed, libObs.value], [true, { ok: true }]);
}

void test('Rust in-crate #[test] fn tenet_observe() observes pub(crate) and private helpers inside OS sandbox', async (context: TestContext) => {
  const check = buildInCrateCheck();

  assertInCrateDetection(check);
  assertRustCommands();

  if (!(await canRun())) {
    context.skip('Isolated sandbox is unavailable');

    return;
  }

  await assertInCrateSandbox(check);
  await assertLibSubjectInCrate();
});

function buildLargeFacadeSource(header: string): string {
  const paddingLine = '// seam facade padding line to push file past 12 KiB limit for truncation.\n';

  return `${header}${paddingLine.repeat(240)}`;
}

async function writeLargeFacadeFixture(root: string): Promise<string> {
  const srcDir = `${root}/src`;
  const facadeHeader = 'pub fn public_facade() -> i32 { 7 }\n';
  const largeFacade = buildLargeFacadeSource(facadeHeader);
  const facadeBytes = Buffer.byteLength(largeFacade);

  await mkdir(srcDir, { recursive: true });
  await writeFile(`${srcDir}/worker.rs`, 'pub(crate) fn work() -> i32 { 7 }\n');
  await writeFile(`${srcDir}/lib.rs`, largeFacade);
  assert.ok(facadeBytes > 12_288);

  return facadeHeader;
}

void test('readInventory truncates >12 KiB assets reference files instead of dropping them', async (context: TestContext) => {
  const root = await createProject(context);
  const facadeHeader = await writeLargeFacadeFixture(root);
  const scoped = { ...contract, assets: ['src/lib.rs'], paths: ['src/worker.rs'] };
  const inventory = await readInventory({ contract: scoped, root });
  const sample = inventory.assets['src/lib.rs'] ?? '';
  const startsWithHeader = sample.startsWith(facadeHeader);
  const hasTruncationNote = sample.includes('[truncated: showing first ');
  const isBounded = Buffer.byteLength(sample) <= 12_500;

  assert.deepEqual([startsWithHeader, hasTruncationNote, isBounded], [true, true, true]);
});

function formatStreamResponse(payload: Readonly<Record<string, unknown>>): Response {
  const serialized = JSON.stringify(payload);

  const frames = [
    JSON.stringify({ choices: [{ delta: { content: serialized }, index: 0 }] }),
    JSON.stringify({ choices: [{ delta: {}, finish_reason: 'stop', index: 0 }], usage: { cost: RETURNED_COST } }),
    '[DONE]',
  ];

  const body = frames.map((chunk) => `data: ${chunk}\n\n`).join('');

  return new Response(body, { headers: { 'content-type': 'text/event-stream' } });
}

function buildAuditFixtureReplies(): readonly Readonly<Record<string, unknown>>[] {
  const candidate = { message: 'compute increments its numeric argument.', name: 'worker-compute', paths: ['src/worker.mjs'], rules: '`compute(x)` returns `x + 1` when called through `src/index.mjs`.' };
  const check = { defect: 'export function compute(x) { return x - 1; }\n', expected: 11, good: 'export function compute(x) { return x + 1; }\n', id: 'facade-calls-compute', promise: 'src/index.mjs runWorker delegates to src/worker.mjs compute.', refactor: 'export function compute(x) { return 1 + x; }\n', runtime: 'node', subject: 'src/worker.mjs', test: 'import { runWorker } from "./src/index.mjs"; console.log(JSON.stringify(runWorker(10)));' };

  return [{ contracts: [candidate] }, { checks: [check], open: [] }];
}

function installAuditFetchMock(context: TestContext): readonly string[] {
  const requests: string[] = [];
  const queue = [...buildAuditFixtureReplies()];

  // Mock fetch receives (url, init) matching globalThis.fetch.
  // oxlint-disable-next-line max-params
  context.mock.method(globalThis, 'fetch', (_url: unknown, init?: RequestInit): Response => {
    const rawBody = init?.body;

    if (typeof rawBody === 'string') {
      requests.push(rawBody);
    }

    const nextPayload = queue.shift() ?? { checks: [], open: [] };

    return formatStreamResponse(nextPayload);
  });

  return requests;
}

async function writeAuditSeamFixture(root: string): Promise<string> {
  const srcDir = `${root}/src`;
  const state = `${root}/.tenet-state`;
  const largeIndex = buildLargeFacadeSource(FACADE_HEADER);

  await mkdir(srcDir, { recursive: true });
  await mkdir(state, { recursive: true });
  await writeFile(`${srcDir}/worker.mjs`, 'export function compute(x) { return x + 1; }\n');
  await writeFile(`${srcDir}/index.mjs`, largeIndex);
  await writeFile(`${srcDir}/helper.mjs`, 'export const HELPER_TAG = "seam-helper";\n');

  return state;
}

function readUserMessageContent(rawBody: string): string {
  const body = decodeJsonRecord(rawBody);
  const messages = Option.flatMap(body, (item) => decodeRawList(item.messages));

  if (Option.isNone(messages)) {
    return '';
  }

  const userEntry = messages.value.find((entry) => entry.role === 'user');
  const content = decodeString(userEntry?.content);

  return Option.getOrElse(content, () => '');
}

function readVerifySamples(rawBody: string): Readonly<Record<string, string>> {
  const userContent = readUserMessageContent(rawBody);
  const packet = decodeJsonRecord(userContent);

  if (Option.isNone(packet)) {
    return {};
  }

  const decoded = decodeSampleMap(packet.value.reference_samples);

  return Option.getOrElse(decoded, () => ({}));
}

function assertAuditSeamForwarding({ captured, facadeHeader, response }: VerifyAssertInput): void {
  const rawBody = captured.at(1) ?? '';
  const samples = readVerifySamples(rawBody);
  const keys = Object.keys(samples).toSorted();
  const indexSample = samples['src/index.mjs'] ?? '';
  const helperSample = samples['src/helper.mjs'] ?? '';
  const hasHeader = indexSample.startsWith(facadeHeader);
  const isTruncated = indexSample.includes('[truncated: showing first ');
  const hasHelper = helperSample.includes('seam-helper');
  const outcome = response.modules.at(0)?.findings.at(0)?.outcome;

  assert.deepEqual([keys, hasHeader, isTruncated, hasHelper, outcome], [['src/helper.mjs', 'src/index.mjs'], true, true, true, 'passed']);
}

async function runAuditSeamScenario(context: TestContext): Promise<void> {
  const root = await createProject(context);
  const state = await writeAuditSeamFixture(root);
  const captured = installAuditFetchMock(context);
  const previousKey = env.OPENROUTER_API_KEY;

  env.OPENROUTER_API_KEY = 'fixture';

  try {
    const module = { docs: [], files: ['src/worker.mjs'], manifests: [], name: 'worker', relatedFiles: ['src/worker.mjs', 'src/index.mjs', 'src/index.mjs', 'src/helper.mjs'] };
    const response = await runAudit({ maxRequests: 4, mode: 'audit', modules: [module], root, runtimePaths: ['src/worker.mjs', 'src/index.mjs', 'src/helper.mjs'], state });

    assertAuditSeamForwarding({ captured, facadeHeader: FACADE_HEADER, response });
  } finally {
    env.OPENROUTER_API_KEY = previousKey ?? '';
  }
}

void test('runAudit forwards deduplicated module.relatedFiles into candidate assets and verification samples', async (context: TestContext) => {
  if (!(await canRun())) {
    context.skip('Isolated sandbox is unavailable');

    return;
  }

  await runAuditSeamScenario(context);
});
