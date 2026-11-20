import { access } from 'node:fs/promises';
import { Effect } from 'effect';
import { CheckerError } from '../checker-error.mts';
import { tryOperation } from '../try-operation.mts';
import { runFile } from './host-tool.mts';

const BWRAP = '/usr/bin/bwrap';
const PRLIMIT = '/usr/bin/prlimit';
const SANDBOX_EXEC = '/usr/bin/sandbox-exec';
const PREFLIGHT_TIMEOUT_MS = 15_000;
const OUTPUT_BYTES = 16_384;
const SYSTEM_PATH = '/usr/bin:/bin';

const MACOS_PREFLIGHT_PROFILE =
  '(version 1)(deny default)(allow process-exec)(allow file-read*)(allow file-map-executable)(allow sysctl-read)(deny network*)';

const LINUX_PREFLIGHT_ARGS = [
  ['--unshare-all', '--new-session', '--die-with-parent', '--clearenv'],
  ['--cap-drop', 'ALL', '--ro-bind', '/', '/'],
  ['--proc', '/proc', '--dev', '/dev'],
  ['--size', '67108864', '--tmpfs', '/tmp'],
  ['--', PRLIMIT, '--cpu=2', '--', '/usr/bin/true'],
].flat();

async function checkLinuxTools(): Promise<void> {
  await access(BWRAP);
  await access(PRLIMIT);
}

async function ensureLinuxTools(): Promise<void> {
  const check = tryOperation({
    message: 'an isolated Linux runner with bubblewrap and prlimit is required',
    run: checkLinuxTools,
  });

  await Effect.runPromise(check);
}

async function verifyLinuxSandbox(): Promise<void> {
  await ensureLinuxTools();

  const result = await runFile({
    args: LINUX_PREFLIGHT_ARGS,
    file: BWRAP,
    options: {
      encoding: 'utf-8',
      env: { PATH: SYSTEM_PATH },
      killSignal: 'SIGKILL',
      maxBuffer: OUTPUT_BYTES,
      timeout: PREFLIGHT_TIMEOUT_MS,
    },
  });

  if (result.status !== 0 && result.stderr.startsWith('bwrap: Unknown option --size')) {
    throw new CheckerError({
      message: 'install bubblewrap 0.7 or later to enforce the 64 MiB temporary filesystem limit',
    });
  }

  if (result.status !== 0) {
    throw new CheckerError({
      message: 'the Linux bubblewrap sandbox is unavailable in this environment',
    });
  }
}

async function checkMacosTool(): Promise<void> {
  await access(SANDBOX_EXEC);
}

async function ensureMacosTool(): Promise<void> {
  const check = tryOperation({
    message: 'macOS sandbox-exec is unavailable in this environment',
    run: checkMacosTool,
  });

  await Effect.runPromise(check);
}

async function verifyMacosSandbox(): Promise<void> {
  await ensureMacosTool();

  const result = await runFile({
    args: ['-p', MACOS_PREFLIGHT_PROFILE, '/usr/bin/true'],
    file: SANDBOX_EXEC,
    options: {
      encoding: 'utf-8',
      env: { PATH: SYSTEM_PATH },
      killSignal: 'SIGKILL',
      maxBuffer: OUTPUT_BYTES,
      timeout: PREFLIGHT_TIMEOUT_MS,
    },
  });

  if (result.status !== 0) {
    throw new CheckerError({
      message: 'the macOS sandbox-exec runner is unavailable in this environment',
    });
  }
}

/** Verify that the host can run isolated behavior checks before spending a model request. */
export async function assertSandboxAvailable(): Promise<void> {
  if (process.platform === 'linux') {
    await verifyLinuxSandbox();

    return;
  }

  if (process.platform === 'darwin') {
    await verifyMacosSandbox();

    return;
  }

  throw new CheckerError({
    message: 'an isolated Linux or macOS runner is required for behavior checks',
  });
}
