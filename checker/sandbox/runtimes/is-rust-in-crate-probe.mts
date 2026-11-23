import type { Check } from '../../types.d.mts';

const TEST_ATTR_PATTERN = /#\[test\]/v;
const OBSERVE_FN_PATTERN = /fn\s+tenet_observe\s*\(/v;

type RustInCrateProbeInput = Readonly<{
  check: Check;
  files: ReadonlyMap<string, string>;
}>;

/** Return true when a Rust probe defines `#[test] fn tenet_observe()` against a library crate. */
export function isRustInCrateProbe({ check, files }: RustInCrateProbeInput): boolean {
  if (check.runtime !== 'rust' || !files.has('src/lib.rs')) {
    return false;
  }

  const code = check.test ?? '';

  return TEST_ATTR_PATTERN.test(code) && OBSERVE_FN_PATTERN.test(code);
}
