import { strict as assert } from 'node:assert';
import { test } from 'node:test';
import { applySkepticOutput, parseAuditCandidates, selectAuditTier } from '../audit/index.mts';
import type { Finding } from '../types.d.mts';

void test('parseAuditCandidates normalizes slugs, keeps valid paths, and formats CONTRACT.md', () => {
  const raw = {
    contracts: [
      {
        message: 'Preserve trailing zeros in formatted decimals.',
        name: 'Format_Decimal Rounding!',
        paths: ['src/format.py', '../outside.py'],
        rules: '`format_decimal` must keep trailing zeros.',
      },
      {
        message: 'Duplicate slug gets prefixed with module name.',
        name: 'Format_Decimal Rounding!',
        paths: [],
        rules: '`parse_decimal` rejects empty strings.',
      },
      {
        message: '   ',
        name: 'empty-message',
        paths: ['src/format.py'],
        rules: 'Ignored because message is blank.',
      },
    ],
  };

  const candidates = parseAuditCandidates({
    allowedPaths: ['src/format.py', 'src/parse.py'],
    manifestPaths: ['pyproject.toml'],
    moduleName: 'src/core',
    raw,
  });

  assert.equal(candidates.length, 2);
  assert.equal(candidates[0]?.name, 'format-decimal-rounding');
  assert.deepEqual(candidates[0]?.paths, ['pyproject.toml', 'src/format.py']);
  assert.equal(candidates[1]?.name, 'src-core-format-decimal-rounding');
  assert.deepEqual(candidates[1]?.paths, ['pyproject.toml', 'src/format.py', 'src/parse.py']);

  const firstDoc = candidates[0]?.document ?? '';
  const hasFrontmatter = firstDoc.startsWith('---\nname: format-decimal-rounding\n');

  assert.ok(hasFrontmatter);
});

void test('parseAuditCandidates returns empty list for malformed payloads', () => {
  const candidates = parseAuditCandidates({
    allowedPaths: ['src/app.ts'],
    manifestPaths: [],
    moduleName: 'src',
    raw: { unexpected: true },
  });

  assert.deepEqual(candidates, []);
});

type SampleFindingInput = Readonly<{
  check: string;
  message: string;
  outcome: Finding['outcome'];
}>;

function makeFinding({ check, message, outcome }: SampleFindingInput): Finding {
  return {
    check,
    contract: 'demo',
    cost_usd: 0.01,
    message,
    outcome,
    requested: true,
    reused: false,
    subject: 'src/app.js',
  };
}

function buildSkepticFixture(): Readonly<{ findings: readonly Finding[]; raw: unknown }> {
  const raw = {
    reviews: [
      { check: 'medium-bug', reason: 'Minor edge case.', verdict: 'medium' },
      { check: 'false-alarm', reason: 'Existing test asserts this quirk.', verdict: 'reject' },
      { check: 'critical-bug', reason: 'Mutates caller array in place.', verdict: 'critical' },
      { check: 'high-bug', reason: 'Breaks round-trip contract.', verdict: 'high' },
    ],
  };

  const findings: readonly Finding[] = [
    makeFinding({ check: 'medium-bug', message: 'Edge mismatch.', outcome: 'failed' }),
    makeFinding({ check: 'false-alarm', message: 'Expected error.', outcome: 'failed' }),
    makeFinding({ check: 'critical-bug', message: 'Caller input mutated.', outcome: 'failed' }),
    makeFinding({ check: 'high-bug', message: 'Round-trip failed.', outcome: 'failed' }),
    makeFinding({ check: 'passing-guard', message: 'Behavior holds.', outcome: 'passed' }),
  ];

  return { findings, raw };
}

void test('applySkepticOutput ranks confirmed witnesses by severity and downgrades false positives', () => {
  const { findings, raw } = buildSkepticFixture();
  const updated = applySkepticOutput({ findings, raw });
  const rejected = updated.find((row) => row.check === 'false-alarm');
  const rejectedNote = rejected?.message ?? '';
  const ordered = updated.slice(0, 3).map((row) => `${row.check}:${row.message}`);

  const isDismissed = rejectedNote.startsWith(
    'dismissed by skeptic: Existing test asserts this quirk.',
  );

  const passing = updated.find((row) => row.check === 'passing-guard');

  assert.deepEqual(ordered, [
    'critical-bug:[critical] Caller input mutated.',
    'high-bug:[high] Round-trip failed.',
    'medium-bug:[medium] Edge mismatch.',
  ]);
  assert.deepEqual(
    [rejected?.outcome, isDismissed, passing?.outcome],
    ['unresolved', true, 'passed'],
  );
});

void test('selectAuditTier chooses reasoning tier for deep mode and honors overrides', () => {
  const standard = selectAuditTier({ agentModel: '', auditModel: '', deep: false });
  const deep = selectAuditTier({ agentModel: '', auditModel: '', deep: true });

  const overridden = selectAuditTier({
    agentModel: 'openai/gpt-6-luna',
    auditModel: 'anthropic/claude-sonnet-4.6',
    deep: true,
  });

  assert.deepEqual(standard, { effort: 'low', model: 'openai/gpt-6-luna' });
  assert.deepEqual(deep, { effort: 'medium', model: 'openai/gpt-6.1-terra' });
  assert.deepEqual(overridden, { effort: 'medium', model: 'anthropic/claude-sonnet-4.6' });
});
