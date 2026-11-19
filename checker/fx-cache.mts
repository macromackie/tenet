import { spawn } from 'node:child_process';
import { once } from 'node:events';
import { access, cp, mkdir, mkdtemp, realpath, rename, rm } from 'node:fs/promises';
import { homedir } from 'node:os';
import { basename, dirname, isAbsolute, join, relative, resolve, sep } from 'node:path';
import { cwd, env } from 'node:process';
import { Effect, Result } from 'effect';
import { CheckerError } from './checker-error.mts';
import { tryOperation } from './try-operation.mts';

type PublishInput = { readonly directory: string; readonly destination: string };
type EnsureInstallationInput = { readonly cache: string; readonly destination: string };
const LIBFX_VERSION = '0.0.17';
const INSTALL_TIMEOUT_MS = 90_000;
const alreadyPublishedCodes = ['EEXIST', 'ENOTEMPTY'] as const;
const publishedCodes: ReadonlySet<string> = new Set(alreadyPublishedCodes);

function readCode(value: unknown): string {
  if (value instanceof Error && 'code' in value && typeof value.code === 'string') {
    return value.code;
  }

  return '';
}

async function readRealPath(path: string): Promise<Result.Result<string, CheckerError>> {
  async function run() {
    return await realpath(path);
  }

  const inspect = tryOperation({ message: 'resolve the fx cache path', run });

  return await inspect.pipe(Effect.result, Effect.runPromise);
}

async function resolveDestination(path: string): Promise<string> {
  const resolved = await readRealPath(path);

  if (Result.isSuccess(resolved)) {
    return resolved.success;
  }

  if (readCode(resolved.failure.cause) !== 'ENOENT') {
    throw resolved.failure;
  }

  const parent = dirname(path);
  const trusted = await resolveDestination(parent);
  const child = basename(path);

  return join(trusted, child);
}

/** Resolve a path, including missing descendants, outside the working tree. */
async function resolveTrustedPath(path: string): Promise<string> {
  const sourceRoot = env.TENET_WORKER_SOURCE_ROOT ?? cwd();
  const source = await realpath(sourceRoot);
  const absolute = resolve(path);
  const destination = await resolveDestination(absolute);
  const relation = relative(source, destination);

  if (isAbsolute(relation) || relation === '..' || relation.startsWith(`..${sep}`)) {
    return destination;
  }

  throw new CheckerError({ message: 'move the fx cache outside the reviewed repository' });
}

async function copyManifest(input: PublishInput): Promise<void> {
  const source = new URL(input.directory, import.meta.url);
  const target = join(input.destination, input.directory);

  await cp(source, target);
}

async function install(directory: string): Promise<void> {
  await copyManifest({ destination: directory, directory: 'package.json' });
  await copyManifest({ destination: directory, directory: 'package-lock.json' });

  const options = {
    HOME: env.HOME,
    HTTPS_PROXY: env.HTTPS_PROXY,
    HTTP_PROXY: env.HTTP_PROXY,
    NODE_EXTRA_CA_CERTS: env.NODE_EXTRA_CA_CERTS,
    NO_PROXY: env.NO_PROXY,
    PATH: env.PATH,
    SSL_CERT_FILE: env.SSL_CERT_FILE,
    npm_config_userconfig: '/dev/null',
  };

  const child = spawn('npm', ['ci', '--omit=dev', '--ignore-scripts', '--no-audit', '--no-fund'], {
    cwd: directory,
    env: options,
    stdio: 'ignore',
    timeout: INSTALL_TIMEOUT_MS,
  });

  const values: unknown[] = await once(child, 'close');

  if (values.at(0) !== 0) {
    throw new CheckerError({ message: 'npm could not install the pinned fx runtime' });
  }
}

async function publish({ directory, destination }: PublishInput): Promise<void> {
  async function run() {
    await rename(directory, destination);
  }

  const promote = tryOperation({ message: 'publish the pinned fx installation', run });
  const result = await promote.pipe(Effect.result, Effect.runPromise);

  if (Result.isFailure(result)) {
    const code = readCode(result.failure.cause);

    if (!publishedCodes.has(code)) {
      throw result.failure;
    }
  }
}

/** Install and publish the pinned package outside the working tree. */
async function ensureInstallation({ cache, destination }: EnsureInstallationInput): Promise<void> {
  await mkdir(cache, { mode: 0o700, recursive: true });

  const prefix = join(cache, 'install-');
  const temporary = await mkdtemp(prefix);

  try {
    await install(temporary);
    await publish({ destination, directory: temporary });
  } finally {
    await rm(temporary, { force: true, recursive: true });
  }
}

/** Return canonical host-owned paths for the pinned SDK installation. */
async function resolveInstallation(): Promise<{
  cache: string;
  destination: string;
  entry: string;
}> {
  const userHome = homedir();
  const home = env.XDG_CACHE_HOME ?? join(userHome, '.cache');
  const directory = join(home, 'tenet', 'libfx');
  const cache = await resolveTrustedPath(directory);
  const destination = join(cache, LIBFX_VERSION);
  const entry = join(destination, 'node_modules', 'libfx', 'node.js');

  return { cache, destination, entry };
}

async function isCached(trusted: string): Promise<boolean> {
  async function run() {
    await access(trusted);
  }

  const read = tryOperation({ message: 'inspect the fx cache', run });
  const cached = await read.pipe(Effect.result, Effect.runPromise);

  if (Result.isSuccess(cached)) {
    return true;
  }

  if (readCode(cached.failure.cause) !== 'ENOENT') {
    throw cached.failure;
  }

  return false;
}

/** Locate or install the pinned library and confine its host cache outside the assessed source. */
export async function resolveLibraryPath(): Promise<string> {
  const { cache, destination, entry } = await resolveInstallation();
  const trusted = await resolveTrustedPath(entry);
  const cached = await isCached(trusted);

  if (cached) {
    return trusted;
  }

  await ensureInstallation({ cache, destination });

  return await resolveTrustedPath(entry);
}
