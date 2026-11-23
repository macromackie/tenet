import { dirname } from 'node:path';
import { CheckerError } from '../checker-error.mts';
import type { Check } from '../types.d.mts';

const GO_TEST_PROBE_MARKER = 'func TestTenetObserve(';
const GO_TEST_PROBE_FILE = 'tenet_observe_test.go';

const PROBE_FILES = {
  go: '.tenet_observe/main.go',
  node: '.tenet-observe.mjs',
  python: '.tenet-observe.py',
  rust: '.tenet-observe.rs',
} as const;

function readGoSubjectDir(subject: string): string {
  if (subject.length === 0) {
    return '.';
  }

  return dirname(subject).replaceAll('\\', '/');
}

function resolveGoProbeEntry(check: Check): string {
  if (!(check.test?.includes(GO_TEST_PROBE_MARKER) ?? false)) {
    return PROBE_FILES.go;
  }

  const normalized = readGoSubjectDir(check.subject);

  if (normalized === '.' || normalized.length === 0) {
    return GO_TEST_PROBE_FILE;
  }

  if (normalized.startsWith('/') || normalized.split('/').includes('..')) {
    throw new CheckerError({ message: 'go test probe subject falls outside the checkout' });
  }

  return `${normalized}/${GO_TEST_PROBE_FILE}`;
}

/** Resolve the relative probe file path inside the staged sandbox work directory. */
export function resolveProbeFile(check: Check): string {
  if (check.runtime === 'go') {
    return resolveGoProbeEntry(check);
  }

  return PROBE_FILES[check.runtime];
}
