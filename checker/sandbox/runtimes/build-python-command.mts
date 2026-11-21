import { basename, dirname } from 'node:path';
import type { Check } from '../../types.d.mts';

type BuildPythonCommandInput = Readonly<{
  binary: string;
  check: Check;
  probeFile: string;
}>;

/** Build a bounded Python probe or unittest invocation shared by OS sandboxes. */
export function buildPythonCommand({
  binary,
  check,
  probeFile,
}: BuildPythonCommandInput): readonly string[] {
  if (check.testFile === undefined) {
    return [binary, '-B', `./${probeFile}`];
  }

  const testDir = dirname(check.testFile);
  const testPattern = basename(check.testFile);

  return [binary, '-B', '-m', 'unittest', 'discover', '-s', `./${testDir}`, '-p', testPattern];
}
