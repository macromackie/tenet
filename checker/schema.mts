import { Option, Schema } from 'effect';
import { CheckerError } from './checker-error.mts';
import { validate } from './sandbox/index.mts';
import type { Inventory } from './types.d.mts';
import { Bundle } from './wire.mts';

type ParseBundleInput = { readonly value: unknown; readonly files: Inventory['files'] };
const decodeBundle = Schema.decodeUnknownOption(Bundle, { onExcessProperty: 'error' });
const MAX_CHECKS = 12;

function readBundle(value: unknown): Bundle {
  const decoded = decodeBundle(value);

  if (Option.isNone(decoded)) {
    throw new CheckerError({ message: 'the checker plan has invalid fields' });
  }

  return decoded.value;
}

/** Validate every checker before freezing its recipe or executing a control. */
export function parseBundle({ value, files }: ParseBundleInput): Bundle {
  const { checks, open } = readBundle(value);

  if (checks.length > MAX_CHECKS || open.length > MAX_CHECKS) {
    throw new CheckerError({ message: 'checker plan exceeds the number of allowed checks' });
  }

  const verified = checks.map((check) => validate({ check, files }));
  const identifiers = verified.map((check) => check.id);
  const names: ReadonlySet<string> = new Set(identifiers);

  if (names.size !== verified.length || names.size + open.length === 0) {
    throw new CheckerError({ message: 'checker plan has missing or duplicate checks' });
  }

  return { checks: verified, open };
}
