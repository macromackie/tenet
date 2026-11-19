import { tmpdir } from 'node:os';
import { env as hostEnvironment } from 'node:process';
import { CheckerError } from '../checker-error.mts';
import { runFile } from './host-tool.mts';

const TOOL_TIMEOUT_MS = 5_000;
const OUTPUT_BYTES = 65_536;

/** Query the host's selected compiler without forwarding provider credentials. */
export async function readRustTool(arg: '-Vv' | '--print=sysroot'): Promise<string> {
  const env = {
    CARGO_HOME: hostEnvironment.CARGO_HOME,
    HOME: hostEnvironment.HOME,
    PATH: hostEnvironment.PATH,
    RUSTUP_HOME: hostEnvironment.RUSTUP_HOME,
    RUSTUP_TOOLCHAIN: hostEnvironment.RUSTUP_TOOLCHAIN,
  };

  const cwd = tmpdir();

  const result = await runFile({
    args: [arg],
    file: 'rustc',
    options: {
      cwd,
      encoding: 'utf-8',
      env,
      killSignal: 'SIGKILL',
      maxBuffer: OUTPUT_BYTES,
      timeout: TOOL_TIMEOUT_MS,
    },
  });

  if (result.status !== 0) {
    throw new CheckerError({ message: 'the Rust toolchain is unavailable' });
  }

  return result.stdout.trim();
}
