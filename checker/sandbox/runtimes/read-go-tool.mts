import { realpath, stat } from 'node:fs/promises';
import { homedir, tmpdir } from 'node:os';
import { basename, delimiter, join } from 'node:path';
import { env as hostEnvironment } from 'node:process';
import { Option } from 'effect';
import { CheckerError } from '../../checker-error.mts';
import { runFile } from '../host-tool.mts';
import { runOptional } from '../run-optional.mts';

const TOOL_TIMEOUT_MS = 20_000;
const OUTPUT_BYTES = 65_536;
const GO_BINARIES: ReadonlySet<string> = new Set(['go', 'go.exe']);

function readGorootGoCandidates(): readonly string[] {
  const goroot = hostEnvironment.GOROOT?.trim() ?? '';

  if (goroot.length === 0) {
    return [];
  }

  return [join(goroot, 'bin', 'go')];
}

function readPathGoCandidates(): readonly string[] {
  const rawPath = hostEnvironment.PATH ?? '';

  const dirs = rawPath
    .split(delimiter)
    .map((entry) => entry.trim())
    .filter((entry) => entry.length > 0);

  return dirs.map((dir) => join(dir, 'go'));
}

function readFallbackGoCandidates(): readonly string[] {
  const userHome = homedir();
  const miseDefault = join(userHome, '.local', 'share', 'mise', 'installs', 'go', 'latest', 'bin', 'go');
  const fallbacks = ['/usr/local/go/bin/go', '/opt/homebrew/bin/go', miseDefault];
  const xdgDataHome = hostEnvironment.XDG_DATA_HOME?.trim() ?? '';

  if (xdgDataHome.length === 0) {
    return fallbacks;
  }

  return [...fallbacks, join(xdgDataHome, 'mise', 'installs', 'go', 'latest', 'bin', 'go')];
}

function collectGoCandidates(): readonly string[] {
  return [...readGorootGoCandidates(), ...readPathGoCandidates(), ...readFallbackGoCandidates()];
}

async function readVerifiedGoBinary(candidate: string): Promise<string> {
  const resolved = await realpath(candidate);
  const info = await stat(resolved);
  const name = basename(resolved);

  if (!info.isFile() || !GO_BINARIES.has(name)) {
    throw new CheckerError({ message: 'candidate is not a real go binary' });
  }

  return resolved;
}

async function resolveHostGoBinary(): Promise<string> {
  const candidates = collectGoCandidates();

  for (const candidate of candidates) {
    // Check candidate Go binaries sequentially in strict precedence order.
    // oxlint-disable-next-line no-await-in-loop
    const resolved = await runOptional(async () => await readVerifiedGoBinary(candidate));

    if (Option.isSome(resolved)) {
      return resolved.value;
    }
  }

  return 'go';
}

/** Query the host Go toolchain without forwarding provider credentials. */
export async function readGoTool(args: readonly string[]): Promise<string> {
  const env = {
    GOMODCACHE: hostEnvironment.GOMODCACHE,
    GOPATH: hostEnvironment.GOPATH,
    GOROOT: hostEnvironment.GOROOT,
    HOME: hostEnvironment.HOME,
    PATH: hostEnvironment.PATH,
  };

  const cwd = tmpdir();
  const goBinary = await resolveHostGoBinary();

  const result = await runFile({
    args,
    file: goBinary,
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
