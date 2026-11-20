import { access } from 'node:fs/promises';
import { Result } from 'effect';
import { classifyResult } from '../classify-result.mts';

/** Locate a checker worker asset both when bundled beside worker.mjs and unbundled under fx/. */
export async function resolveWorkerAsset(name: string): Promise<URL> {
  const local = new URL(name, import.meta.url);

  async function probeLocal() {
    await access(local);
  }

  const found = await classifyResult({ message: 'missing local worker asset', run: probeLocal });

  if (Result.isSuccess(found)) {
    return local;
  }

  return new URL(`../${name}`, import.meta.url);
}
