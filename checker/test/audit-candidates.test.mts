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

void test('applySkepticOutput keeps confirmed witnesses and downgrades false positives', () => {
  const raw = {
    reviews: [
      { check: 'real-bug', reason: 'Contradicts docstring.', verdict: 'confirm' },
      { check: 'false-alarm', reason: 'Intentional documented default.', verdict: 'reject' },
    ],
  };

  const findings: readonly Finding[] = [
    {
      check: 'real-bug',
      contract: 'demo',
      cost_usd: 0.01,
      message: 'Observed mismatch.',
      outcome: 'failed',
      requested: true,
      reused: false,
      subject: 'src/app.js',
    },
    {
      check: 'false-alarm',
      contract: 'demo',
      cost_usd: 0.01,
      message: 'Expected error.',
      outcome: 'failed',
      requested: true,
      reused: false,
      subject: 'src/app.js',
    },
    {
      check: 'passing-guard',
      contract: 'demo',
      cost_usd: 0.01,
      message: 'Behavior holds.',
      outcome: 'passed',
      requested: true,
      reused: false,
      subject: 'src/app.js',
    },
  ];

  const updated = applySkepticOutput({ findings, raw });
  const rejectedNote = updated[1]?.message ?? '';

  const isDismissed = rejectedNote.startsWith(
    'dismissed by skeptic: Intentional documented default.',
  );

  assert.equal(updated[0]?.outcome, 'failed');
  assert.equal(updated[1]?.outcome, 'unresolved');
  assert.ok(isDismissed);
  assert.equal(updated[2]?.outcome, 'passed');
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
