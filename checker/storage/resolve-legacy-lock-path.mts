import { dirname, join } from 'node:path';

const LEGACY_LOCK_NAME = 'agent-checks.lock.json';

/** Return the legacy lockfile path `<dir>/agent-checks.lock.json` for a contract path. */
export function resolveLegacyLockPath(contractPath: string): string {
  const contractDir = dirname(contractPath);

  return join(contractDir, LEGACY_LOCK_NAME);
}
