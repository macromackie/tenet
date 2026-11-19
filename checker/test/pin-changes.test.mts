import { strict as assert } from 'node:assert';
import { test } from 'node:test';
import { Option } from 'effect';
import { parsePin, updatePin } from '../review/index.mts';
import type { Check, Inventory, Pin } from '../types.d.mts';

const CHOOSE = 'choose.mjs';
const ENTRY = 'choose.test.mjs';
const OTHER = 'main.mjs';
const GOOD = 'export const choose = (value) => value ?? 7;';
const DEFECT = 'export const choose = (value) => value || 7;';

const CHECK = {
  defect: DEFECT,
  expected: 0,
  good: GOOD,
  id: 'keeps-zero',
  promise: 'Explicit zero wins.',
  refactor: 'export const choose = n => n == null ? 7 : n;',
  runtime: 'node',
  subject: CHOOSE,
  test: 'import { choose } from "./choose.mjs";console.log(JSON.stringify(choose(0)))',
} as const;

type MakePinInput = {
  readonly checks: readonly Check[];
  readonly contractHash?: string;
  readonly shape?: string;
};

function makePin({ checks, contractHash, shape = 'current' }: MakePinInput): Pin {
  return { bundle: { checks, open: [] }, contractHash, protocol: 1, shape };
}

function makeInventory(paths: readonly string[]): Inventory {
  const entries = paths.map((path): readonly [string, string] => [path, GOOD]);
  const files = new Map(entries);

  return {
    assets: {},
    decisions: {},
    files,
    observed: 'snapshot',
    omitted: [],
    runtimeFiles: files,
    shape: 'current',
  };
}

type ReadFixturePinInput = {
  readonly prior: Pin;
  readonly current: Inventory;
  readonly contractHash: string;
};

function readFixturePin({ prior, current, contractHash }: ReadFixturePinInput) {
  const text = JSON.stringify(prior);
  const json: unknown = JSON.parse(text);
  const value = Option.some({ json, text });

  return parsePin({ contractHash, current, value });
}

void test('a rewritten requirement replaces the old assertion even if its subject and ID survive', () => {
  const current = makeInventory([CHOOSE]);
  const previous = makePin({ checks: [CHECK], contractHash: 'old' });

  const inverse = {
    ...CHECK,
    defect: CHECK.good,
    expected: 7,
    good: CHECK.defect,
    promise: 'Zero selects the default.',
  };

  const next = makePin({ checks: [inverse], contractHash: 'new' });
  const loaded = readFixturePin({ contractHash: 'new', current, prior: previous });

  assert.equal(loaded.matching, false);
  assert.ok(loaded.pin);

  const result = updatePin({ files: current.runtimeFiles, next, prior: loaded.pin });

  assert.deepEqual(result.bundle.checks, [inverse]);
});

function makePreviousPin(): Pin {
  const keep = { ...CHECK, id: 'keep', subject: OTHER };
  const removed = { ...CHECK, id: 'removed', subject: 'removed.mjs' };
  const native = { ...CHECK, expected: true, testFile: ENTRY };

  return makePin({
    checks: [keep, removed, native],
    contractHash: 'same',
    shape: 'previous',
  });
}

void test('a missing subject or native entry permits regeneration alongside surviving checks', () => {
  const current = makeInventory([OTHER, CHOOSE]);
  const previous = makePreviousPin();

  const loaded = readFixturePin({ contractHash: 'same', current, prior: previous });

  assert.ok(loaded.pin);

  const next = makePin({ checks: [CHECK], contractHash: 'same' });
  const result = updatePin({ files: current.runtimeFiles, next, prior: loaded.pin });
  const names = result.bundle.checks.map((item) => item.id);

  assert.deepEqual(names, ['keep', 'keeps-zero']);
});

void test('legacy pins replay on the recorded surface and do not seed a different surface', () => {
  const prior = makePin({ checks: [CHECK] });
  const current = makeInventory([CHOOSE]);
  const loaded = readFixturePin({ contractHash: 'same', current, prior });

  assert.ok(loaded.matching);

  const stale = { ...prior, shape: 'older' };
  const next = makePin({ checks: [{ ...CHECK, id: 'replacement' }], contractHash: 'same' });
  const result = updatePin({ files: current.files, next, prior: stale });

  assert.deepEqual(result, next);
});

void test('a reassigned ID follows its new target while stable checks keep their qualified controls', () => {
  const current = makeInventory([CHOOSE, OTHER]);
  const prior = makePin({ checks: [CHECK], contractHash: 'same', shape: 'old' });
  const stable = { ...CHECK, good: 'export function choose(n) { return n ?? 7 }' };
  const retargeted = { ...CHECK, subject: OTHER };

  for (const candidate of [stable, retargeted]) {
    const next = makePin({ checks: [candidate], contractHash: 'same' });
    const result = updatePin({ files: current.files, next, prior });
    const [kept] = result.bundle.checks;

    assert.equal(kept.subject, candidate.subject);
    assert.equal(kept.good, CHECK.good);
  }
});
