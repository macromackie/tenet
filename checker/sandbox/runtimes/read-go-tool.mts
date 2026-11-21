import { tmpdir } from 'node:os';
import { env as hostEnvironment } from 'node:process';
import { CheckerError } from '../../checker-error.mts';
import { runFile } from '../host-tool.mts';

const TOOL_TIMEOUT_MS = 20_000;
const OUTPUT_BYTES = 65_536;

/** Query the host Go toolchain without forwarding provider credentials. */
export async function readGoTool(args: readonly string[]): Promise<string> {
  const env = {
    GOPATH: hostEnvironment.GOPATH,
    GOROOT: hostEnvironment.GOROOT,
    HOME: hostEnvironment.HOME,
    PATH: hostEnvironment.PATH,
  };

  const cwd = tmpdir();

  const result = await runFile({
    args,
    file: 'go',
    options: {
      cwd,
      encoding: 'utf-8',
      env,
      killSignal: 'SIGKILL',
      maxBuffer: OUTPUT_BYTES,
      timeout: TOOL_TIMEOUT_MS,
    },
  });

  if (result.status !== 0 || result.stdout.trim().length === 0) {
    throw new CheckerError({ message: 'the Go toolchain is unavailable' });
  }

  return result.stdout.trim();
}
