import { basename } from 'node:path';

const TEST_SEGMENTS: ReadonlySet<string> = new Set([
  '__snapshots__',
  '__tests__',
  'fixtures',
  'test',
  'testdata',
  'tests',
]);

const TEST_SUFFIXES = [
  '_test.go',
  '_test.py',
  '.spec.cjs',
  '.spec.cts',
  '.spec.js',
  '.spec.mjs',
  '.spec.mts',
  '.spec.ts',
  '.test.cjs',
  '.test.cts',
  '.test.js',
  '.test.mjs',
  '.test.mts',
  '.test.ts',
] as const;

/** Identify test and fixture paths that should stay available for execution without crowding prompts. */
export function isTestOrFixturePath(path: string): boolean {
  const segments = path.split('/');

  if (segments.some((segment) => TEST_SEGMENTS.has(segment))) {
    return true;
  }

  const name = basename(path);

  if (name.startsWith('test_') && name.endsWith('.py')) {
    return true;
  }

  return TEST_SUFFIXES.some((suffix) => name.endsWith(suffix));
}
