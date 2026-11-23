import { readFile, writeFile } from 'node:fs/promises';
import { join } from 'node:path';

type WriteRustLibInput = Readonly<{ inCrateRust: boolean; work: string }>;

const RUST_IN_CRATE_HARNESS =
  '\n#[cfg(test)]\n#[path = "../.tenet-observe.rs"]\nmod __tenet_observe_probe;\n';

/** Write `src/__tenet_lib.rs` from the active `src/lib.rs` variant for in-crate Rust test probes. */
export async function writeRustInCrateLib({ inCrateRust, work }: WriteRustLibInput): Promise<void> {
  if (!inCrateRust) {
    return;
  }

  const libPath = join(work, 'src', 'lib.rs');
  const harnessPath = join(work, 'src', '__tenet_lib.rs');
  const currentLib = await readFile(libPath, 'utf-8');

  await writeFile(harnessPath, `${currentLib}${RUST_IN_CRATE_HARNESS}`);
}
