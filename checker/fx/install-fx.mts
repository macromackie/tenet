import { spawn } from 'node:child_process';
import { once } from 'node:events';
import { cp } from 'node:fs/promises';
import { join } from 'node:path';
import { env } from 'node:process';
import { CheckerError } from '../checker-error.mts';
import { resolveWorkerAsset } from './resolve-asset.mts';

const INSTALL_TIMEOUT_MS = 90_000;

type CopyManifestInput = Readonly<{ destination: string; directory: string }>;

async function copyManifest(input: CopyManifestInput): Promise<void> {
  const source = await resolveWorkerAsset(input.directory);
  const target = join(input.destination, input.directory);

  await cp(source, target);
}

/** Install the pinned libfx package into a staging directory using locked manifests. */
export async function installFxRuntime(directory: string): Promise<void> {
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
