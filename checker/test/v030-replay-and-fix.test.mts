import { strict as assert } from 'node:assert';
import { mkdir, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { test, type TestContext } from 'node:test';
import { applyAuditFix, runAuditFix, runAuditReplay } from '../audit/index.mts';
import { observe } from '../sandbox/index.mts';
import type { Check } from '../types.d.mts';
import { canRun } from './can-run-sandbox.mts';
import { createProject, GOOD, SOURCE } from './test-project.mts';

const REPRO_CODE =
  'import { choose } from "./choose.mjs"; console.log(JSON.stringify(choose(0)));';

async function verifyReplayLifecycle(root: string): Promise<void> {
  const state = join(root, '.tenet-state');

  await mkdir(state, { recursive: true });

  const request = {
    mode: 'replay' as const,
    root,
    runtimePaths: [SOURCE],
    state,
    targets: [
      {
        check: 'keeps-zero',
        contract: 'options',
        repro: { code: REPRO_CODE, expected: '0', runtime: 'node' },
        subject: SOURCE,
      },
    ],
  };

  const beforeFix = await runAuditReplay(request);
  const firstBefore = beforeFix.results.at(0);
  const sourcePath = join(root, SOURCE);

  await writeFile(sourcePath, GOOD);

  const afterFix = await runAuditReplay(request);
  const firstAfter = afterFix.results.at(0);

  assert.deepEqual(
    [firstBefore?.outcome, firstBefore?.observed, firstAfter?.outcome, firstAfter?.observed],
    ['failed', '7', 'passed', '0'],
  );
}

void test('runAuditReplay reports failed before fix and passed after fix with zero LLM calls', async (context: TestContext) => {
  if (!(await canRun())) {
    context.skip('Isolated sandbox is unavailable');

    return;
  }

  const root = await createProject(context);

  await verifyReplayLifecycle(root);
});

async function verifyExtensionlessTsImport(): Promise<void> {
  const files = new Map<string, string>([
    ['src/helper.ts', 'export function greet(name: string): string { return "hi " + name; }\n'],
    ['src/index.ts', 'import { greet } from "./helper";\nexport const answer = greet("ufo");\n'],
  ]);

  const check: Check = {
    defect: '',
    expected: 'hi ufo',
    good: '',
    id: 'extensionless-ts',
    promise: 'Extensionless relative TS imports resolve cleanly under Node.',
    refactor: '',
    runtime: 'node',
    subject: 'src/index.ts',
    test: 'import { answer } from "./src/index"; console.log(JSON.stringify(answer));',
  };

  const observation = await observe({ check, files, qualify: false });

  assert.deepEqual([observation.passed, observation.value], [true, 'hi ufo']);
}

void test('node-resolve-hook resolves extensionless ./helper to ./helper.ts inside the OS sandbox', async (context: TestContext) => {
  if (!(await canRun())) {
    context.skip('Isolated sandbox is unavailable');

    return;
  }

  await verifyExtensionlessTsImport();
});

function verifyFixEditsSchemaAndScope(): void {
  const files = new Map<string, string>([
    ['src/a.ts', 'export const x = 1;\n'],
    ['src/secret.ts', 'export const y = 2;\n'],
  ]);

  const invalidSchema = applyAuditFix({
    allowedFiles: ['src/a.ts'],
    files,
    raw: { summary: 'missing edits array' },
  });

  const outsideScope = applyAuditFix({
    allowedFiles: ['src/a.ts'],
    files,
    raw: {
      edits: [{ find: 'y = 2', path: 'src/secret.ts', replace: 'y = 3' }],
      summary: 'tries to edit file outside group.files',
    },
  });

  const valid = applyAuditFix({
    allowedFiles: ['src/a.ts'],
    files,
    raw: {
      edits: [{ find: 'x = 1', line: 1, path: 'src/a.ts', replace: 'x = 42' }],
      summary: 'fix x value',
    },
  });

  assert.equal(valid.ok, true);

  const patched = valid.files.get('src/a.ts');
  const untouched = files.get('src/a.ts');

  assert.deepEqual(
    [invalidSchema.ok, outsideScope.ok, patched, untouched],
    [false, false, 'export const x = 42;\n', 'export const x = 1;\n'],
  );
}

void test('applyAuditFix validates schema, rejects edits outside group.files, and patches in memory', () => {
  verifyFixEditsSchemaAndScope();
});

async function verifyPrecheckSkipsLlm(root: string): Promise<void> {
  const state = join(root, '.tenet-state');
  const sourcePath = join(root, SOURCE);

  await mkdir(state, { recursive: true });
  await writeFile(sourcePath, GOOD);

  const response = await runAuditFix({
    groups: [
      {
        contracts: [],
        files: [SOURCE],
        guards: [],
        name: 'options',
        witnesses: [
          {
            check: 'keeps-zero',
            contract: 'options',
            message: 'expected 0, observed 7',
            repro: { code: REPRO_CODE, expected: '0', runtime: 'node' },
            subject: SOURCE,
          },
        ],
      },
    ],
    maxRequests: 3,
    mode: 'audit-fix',
    root,
    runtimePaths: [SOURCE],
    state,
  });

  const firstOutcome = response.outcomes.at(0);

  assert.deepEqual(
    [response.requests, response.patches.length, firstOutcome?.status],
    [0, 0, 'already_passing'],
  );
}

void test('runAuditFix pre-check marks already-passing witnesses without spending LLM requests', async (context: TestContext) => {
  if (!(await canRun())) {
    context.skip('Isolated sandbox is unavailable');

    return;
  }

  const root = await createProject(context);

  await verifyPrecheckSkipsLlm(root);
});
