import type { Check } from '../../types.d.mts';
import { buildNodeResolveHookFlags } from './node-resolve-hook.mts';

type BuildNodeCommandInput = Readonly<{
  binary: string;
  check: Check;
  probeFile: string;
}>;

const NODE_FLAGS = [
  '--disable-wasm-trap-handler',
  '--max-old-space-size=96',
  '--v8-pool-size=2',
] as const;

/** Build a bounded Node.js probe or native test invocation shared by OS sandboxes. */
export function buildNodeCommand({
  binary,
  check,
  probeFile,
}: BuildNodeCommandInput): readonly string[] {
  const hookFlags = buildNodeResolveHookFlags();

  if (check.testFile === undefined) {
    return [binary, ...NODE_FLAGS, ...hookFlags, `./${probeFile}`];
  }

  return [
    binary,
    ...NODE_FLAGS,
    ...hookFlags,
    '--test',
    '--test-reporter=tap',
    '--',
    `./${check.testFile}`,
  ];
}
