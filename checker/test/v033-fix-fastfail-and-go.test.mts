import { strict as assert } from 'node:assert';
import { chmod, mkdir, realpath, symlink, writeFile } from 'node:fs/promises';
import { delimiter, join } from 'node:path';
import { env } from 'node:process';
import { test, type TestContext } from 'node:test';
import { runAuditFix } from '../audit/index.mts';
import { readGoTool, resolveProjectDeps } from '../sandbox/index.mts';
import { canRun } from './can-run-sandbox.mts';
import { createProject } from './test-project.mts';

type EnvKey = 'GOROOT' | 'OPENROUTER_API_KEY' | 'PATH';
type RestoreEnvInput = Readonly<{ key: EnvKey; previous?: string }>;
type WriteScriptInput = Readonly<{ filePath: string; script: string }>;
type PatchedGoEnvInput<Value> = Readonly<{ goroot?: string; path: string; run: () => Promise<Value> }>;
type RunWithApiKeyInput<Value> = Readonly<{ run: () => Promise<Value> }>;
type ShimDirs = Readonly<{ managerDir: string; realDir: string; shimDir: string }>;

const UNRESOLVED_WITNESS = {
  check: 'missing-repro-env',
  contract: 'env-check',
  message: 'environment toolchain unavailable',
  subject: 'src/calc.mjs',
};

const BUGGY_CALC = 'export function addTen(n) { return n + 9; }\n';

const FAILING_WITNESS = {
  check: 'add-ten',
  contract: 'calc',
  message: 'expected 15, observed 14',
  repro: {
    code: 'import { addTen } from "./src/calc.mjs"; console.log(JSON.stringify(addTen(5)));',
    expected: '15',
    runtime: 'node',
  },
  subject: 'src/calc.mjs',
};

const CALC_FIX_REPLY = {
  edits: [
    {
      find: 'export function addTen(n) { return n + 9; }',
      line: 1,
      path: 'src/calc.mjs',
      replace: 'export function addTen(n) { return n + 10; }',
    },
  ],
  summary: 'Fix addTen off-by-one.',
};

function restoreEnvKey({ key, previous }: RestoreEnvInput): void {
  if (previous === undefined) {
    Reflect.deleteProperty(env, key);

    return;
  }

  Reflect.set(env, key, previous);
}

async function runWithFixtureApiKey<Value>({ run }: RunWithApiKeyInput<Value>): Promise<Value> {
  const previous = env.OPENROUTER_API_KEY;
  env.OPENROUTER_API_KEY = 'fixture';

  try {
    return await run();
  } finally {
    restoreEnvKey({ key: 'OPENROUTER_API_KEY', previous });
  }
}

function buildSseReply(payload: Readonly<Record<string, unknown>>): Response {
  const content = JSON.stringify(payload);
  const chunk = JSON.stringify({ choices: [{ delta: { content }, index: 0 }] });
  const done = JSON.stringify({ choices: [{ delta: {}, finish_reason: 'stop', index: 0 }], usage: { cost: 0.0005 } });
  const body = `data: ${chunk}\n\ndata: ${done}\n\ndata: [DONE]\n\n`;

  return new Response(body, { headers: { 'content-type': 'text/event-stream' } });
}

async function createCalcProject(root: string): Promise<string> {
  const srcDir = join(root, 'src');
  const state = join(root, '.tenet-state');
  const calcPath = join(srcDir, 'calc.mjs');

  await mkdir(srcDir, { recursive: true });
  await mkdir(state, { recursive: true });
  await writeFile(calcPath, BUGGY_CALC);

  return state;
}

function installCountingRequestMock(context: TestContext): Readonly<{ readCount: () => number }> {
  let count = 0;

  function readCount(): number {
    return count;
  }

  context.mock.method(globalThis, 'fetch', (): Response => {
    count += 1;

    return buildSseReply({ edits: [], summary: 'should never be called' });
  });

  return { readCount };
}

function installCalcFixRequestMock(context: TestContext): readonly string[] {
  const bodies: string[] = [];

  // Mock fetch signature matches globalThis.fetch(input, init).
  // oxlint-disable-next-line max-params
  context.mock.method(globalThis, 'fetch', (_url: unknown, init?: RequestInit): Response => {
    if (typeof init?.body === 'string') {
      bodies.push(init.body);
    }

    return buildSseReply(CALC_FIX_REPLY);
  });

  return bodies;
}

void test('runAuditFix fast-fails unresolved pre-check witnesses with zero LLM requests', async (context: TestContext) => {
  const root = await createProject(context);
  const state = await createCalcProject(root);
  const requestSpy = installCountingRequestMock(context);
  const group = { contracts: [], files: ['src/calc.mjs'], guards: [], name: 'env-only', witnesses: [UNRESOLVED_WITNESS] };

  async function executeFix() {
    return await runAuditFix({ groups: [group], maxRequests: 5, mode: 'audit-fix', root, runtimePaths: ['src/calc.mjs'], state });
  }

  const response = await runWithFixtureApiKey({ run: executeFix });
  const callCount = requestSpy.readCount();
  const outcome = response.outcomes.at(0);

  assert.deepEqual([callCount, response.requests, response.costUsd, response.patches.length], [0, 0, 0, 0]);
  assert.deepEqual([outcome?.check, outcome?.status], ['missing-repro-env', 'unresolved']);
});

type MixedAssertInput = Readonly<{
  capturedBodies: readonly string[];
  response: Awaited<ReturnType<typeof runAuditFix>>;
}>;

function assertMixedGroupOutcome({ capturedBodies, response }: MixedAssertInput): void {
  const statuses = response.outcomes.map((item) => [item.check, item.status]);
  const promptBody = capturedBodies.at(0) ?? '';
  const hasUnresolvedInPrompt = promptBody.includes('missing-repro-env');
  const hasFailingInPrompt = promptBody.includes('add-ten');

  assert.deepEqual([hasUnresolvedInPrompt, hasFailingInPrompt], [false, true]);
  assert.deepEqual([response.requests, response.patches.length, statuses], [1, 1, [['missing-repro-env', 'unresolved'], ['add-ten', 'fixed']]]);
}

async function runMixedGroupScenario(context: TestContext): Promise<void> {
  const root = await createProject(context);
  const state = await createCalcProject(root);
  const capturedBodies = installCalcFixRequestMock(context);
  const group = { contracts: [], files: ['src/calc.mjs'], guards: [], name: 'mixed-group', witnesses: [UNRESOLVED_WITNESS, FAILING_WITNESS] };

  async function executeFix() {
    return await runAuditFix({ groups: [group], maxRequests: 3, mode: 'audit-fix', root, runtimePaths: ['src/calc.mjs'], state });
  }

  const response = await runWithFixtureApiKey({ run: executeFix });

  assertMixedGroupOutcome({ capturedBodies, response });
}

void test('runAuditFix preserves precheckUnresolved alongside fixed witnesses in mixed groups', async (context: TestContext) => {
  if (!(await canRun())) {
    context.skip('Isolated sandbox is unavailable');

    return;
  }

  await runMixedGroupScenario(context);
});

async function writeExecutable({ filePath, script }: WriteScriptInput): Promise<void> {
  await writeFile(filePath, script);
  await chmod(filePath, 0o755);
}

async function runWithPatchedGoEnv<Value>({ goroot, path, run }: PatchedGoEnvInput<Value>): Promise<Value> {
  const previousGoroot = env.GOROOT;
  const previousPath = env.PATH;

  restoreEnvKey({ key: 'GOROOT', previous: goroot });

  env.PATH = path;

  try {
    return await run();
  } finally {
    restoreEnvKey({ key: 'GOROOT', previous: previousGoroot });
    restoreEnvKey({ key: 'PATH', previous: previousPath });
  }
}

async function createGoDirs(root: string): Promise<ShimDirs> {
  const shimDir = join(root, 'shims');
  const managerDir = join(root, 'manager');
  const realDir = join(root, 'real-go');

  await mkdir(shimDir, { recursive: true });
  await mkdir(managerDir, { recursive: true });
  await mkdir(realDir, { recursive: true });

  return { managerDir, realDir, shimDir };
}

async function createShimAndRealGo(root: string): Promise<string> {
  const { managerDir, realDir, shimDir } = await createGoDirs(root);
  const miseBinary = join(managerDir, 'mise');
  const shimGo = join(shimDir, 'go');
  const realGo = join(realDir, 'go');

  await writeExecutable({ filePath: miseBinary, script: '#!/bin/sh\necho "No version is set for shim: go" >&2\nexit 1\n' });
  await symlink(miseBinary, shimGo);
  await writeExecutable({ filePath: realGo, script: '#!/bin/sh\necho "go version go1.24.0-shim-test"\n' });

  return `${shimDir}${delimiter}${realDir}`;
}

async function readVersion(): Promise<string> {
  return await readGoTool(['version']);
}

void test('readGoTool skips non-go shim symlinks earlier on PATH and runs the real go binary', async (context: TestContext) => {
  const root = await createProject(context);
  const patchedPath = await createShimAndRealGo(root);
  const output = await runWithPatchedGoEnv({ path: patchedPath, run: readVersion });

  assert.equal(output, 'go version go1.24.0-shim-test');
});

async function assertInRootVenvResolved(context: TestContext): Promise<void> {
  const projectRoot = await createProject(context);
  const sitePackages = join(projectRoot, 'venv', 'lib', 'python3.12', 'site-packages');

  await mkdir(sitePackages, { recursive: true });

  const resolvedDeps = await resolveProjectDeps({ projectRoot, runtime: 'python' });
  const canonicalExpected = await realpath(sitePackages);

  assert.equal(resolvedDeps.pythonSitePackages, canonicalExpected);
}

async function assertOutOfRootVenvRejected(context: TestContext): Promise<void> {
  const escapedRoot = await createProject(context);
  const outsideRoot = await createProject(context);
  const outsideVenv = join(outsideRoot, 'external-venv');
  const outsideSitePackages = join(outsideVenv, 'lib', 'python3.12', 'site-packages');
  const venvLink = join(escapedRoot, 'venv');

  await mkdir(outsideSitePackages, { recursive: true });
  await symlink(outsideVenv, venvLink);

  const rejectedDeps = await resolveProjectDeps({ projectRoot: escapedRoot, runtime: 'python' });

  assert.equal(rejectedDeps.pythonSitePackages, undefined);
}

void test('resolveProjectDeps discovers in-root venv site-packages while rejecting out-of-root symlinks', async (context: TestContext) => {
  await assertInRootVenvResolved(context);
  await assertOutOfRootVenvRejected(context);
});
