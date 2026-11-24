import { strict as assert } from 'node:assert';
import { mkdir, writeFile } from 'node:fs/promises';
import { test, type TestContext } from 'node:test';
import { Result } from 'effect';
import {
  applySkepticOutput,
  parseAuditCandidates,
  resolveReplayCheck,
  runAuditReplay,
} from '../../audit/index.mts';
import { validate } from '../../sandbox/index.mts';
import { writeAtomic, writePinScripts } from '../../storage/index.mts';
import { canRun } from '../can-run-sandbox.mts';
import { check, contract, createProject, SOURCE } from '../test-project.mts';

const DEMO_LOCK = '.contracts/001-demo/scripts/.lock.json';

type ReplayTargetInput = Parameters<typeof resolveReplayCheck>[0]['target'];

function makeDemoPin() {
  return { bundle: { checks: [check], open: [] }, contractHash: contract.hash, protocol: 1 as const, shape: 'shape-1' };
}

async function prepareDemoPin(root: string): Promise<Readonly<{ editedScript: string; state: string }>> {
  const state = `${root}/.tenet-state`;
  const editedScript = 'import {choose} from "./choose.mjs"; console.log(JSON.stringify(0));\n';

  await mkdir(state, { recursive: true });

  const pin = makeDemoPin();
  const materialized = await writePinScripts({ lockPath: DEMO_LOCK, pin, root });

  await writeAtomic({ path: DEMO_LOCK, root, value: materialized.diskPin });
  await writeFile(`${root}/.contracts/001-demo/scripts/keeps-zero.mjs`, editedScript);

  return { editedScript, state };
}

async function assertDemoReplay(
  input: Readonly<{ root: string; state: string; target: ReplayTargetInput }>,
): Promise<void> {
  if (!(await canRun())) {
    return;
  }

  const replay = await runAuditReplay({
    mode: 'replay', root: input.root, runtimePaths: [SOURCE], state: input.state, targets: [input.target],
  });

  assert.equal(replay.results[0].outcome, 'passed');
}

async function verifyPinRootReplay(root: string): Promise<void> {
  const { editedScript, state } = await prepareDemoPin(root);

  const rootTarget: ReplayTargetInput = {
    check: 'keeps-zero', contract: 'demo', lockPath: DEMO_LOCK, pinRoot: 'root', subject: SOURCE,
  };

  const fromRoot = await resolveReplayCheck({ root, state, target: rootTarget });
  const fromState = await resolveReplayCheck({ root, state, target: { ...rootTarget, pinRoot: 'state' } });
  const rootOk = Result.isSuccess(fromRoot);
  const stateFailed = Result.isFailure(fromState);

  assert.deepEqual([rootOk, stateFailed], [true, true]);

  if (Result.isSuccess(fromRoot)) {
    assert.deepEqual([fromRoot.success.test, fromRoot.success.scriptFile], [editedScript, 'keeps-zero.mjs']);
  }

  await assertDemoReplay({ root, state, target: rootTarget });
}

void test('resolveReplayCheck honors pinRoot root and hydrates scripts/.lock.json including manual edits', async (context: TestContext) => {
  const root = await createProject(context);

  await verifyPinRootReplay(root);
});

function buildSkepticFindings() {
  const base = {
    check: 'demo-check', contract: 'demo', cost_usd: 0.01, message: 'Unexpected default.',
    outcome: 'failed' as const, requested: true, reused: false, subject: 'src/app.mjs',
  };

  return [
    { ...base, check: 'crit-check' },
    { ...base, brownfield_unverified: true, check: 'bf-high' },
    { ...base, check: 'med-check' },
    { ...base, check: 'rejected-check' },
    { ...base, check: 'pass-check', outcome: 'passed' as const },
  ];
}

void test('applySkepticOutput populates structured severity alongside [severity] message prefix', () => {
  const findings = buildSkepticFindings();

  const reviewed = applySkepticOutput({
    findings,
    raw: {
      reviews: [
        { check: 'crit-check', reason: 'Corrupts data.', verdict: 'critical' },
        { check: 'bf-high', reason: 'Confirmed zero bug.', verdict: 'high' },
        { check: 'med-check', reason: 'Edge-case mismatch.', verdict: 'medium' },
        { check: 'rejected-check', reason: 'Intentional quirk.', verdict: 'reject' },
      ],
    },
  });

  const entries = reviewed.map((row): readonly [string, (typeof reviewed)[number]] => [row.check, row]);
  const reviewedRows = new Map(entries);
  const critRow = reviewedRows.get('crit-check');
  const bfRow = reviewedRows.get('bf-high');
  const medRow = reviewedRows.get('med-check');
  const rejectedRow = reviewedRows.get('rejected-check');
  const passRow = reviewedRows.get('pass-check');

  assert.deepEqual(
    [
      critRow?.severity, critRow?.message, bfRow?.severity, bfRow?.message,
      medRow?.severity, medRow?.message, rejectedRow?.severity === undefined, passRow?.severity === undefined,
    ],
    [
      'critical', '[critical] Unexpected default.', 'high', '[high] Unexpected default.',
      'medium', '[medium] Unexpected default.', true, true,
    ],
  );
});

void test('parseAuditCandidates stages lockPath under scripts/.lock.json', () => {
  const candidates = parseAuditCandidates({
    allowedPaths: ['src/app.ts'], manifestPaths: [], moduleName: 'src',
    raw: { contracts: [{ message: 'Keep zero.', name: 'explicit-zero', paths: ['src/app.ts'], rules: 'Preserve explicit zero.' }] },
  });

  assert.equal(candidates[0].lockPath, 'audit-pins/explicit-zero/scripts/.lock.json');
});

void test('validate accepts slim pin checks only when fromPin is true', () => {
  const files = new Map<string, string>([[SOURCE, check.good]]);

  const slimCheck = {
    controlHash: 'abc123',
    expected: check.expected,
    id: check.id,
    promise: check.promise,
    runtime: check.runtime,
    scriptFile: 'keeps-zero.mjs',
    subject: SOURCE,
    test: check.test,
  };

  assert.throws(() => validate({ check: slimCheck, files, fromPin: false }));

  const validated = validate({ check: slimCheck, files, fromPin: true });

  assert.deepEqual(
    [validated.controlHash, validated.scriptFile, validated.good === undefined],
    ['abc123', 'keeps-zero.mjs', true],
  );
});
