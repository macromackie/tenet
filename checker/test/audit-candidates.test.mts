import { strict as assert } from 'node:assert';
import { test } from 'node:test';
import { parseAuditCandidates } from '../audit/index.mts';

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
