import { dirname, join } from 'node:path';

const SCRIPTS_DIR_NAME = 'scripts';
const LOCK_FILE_NAME = '.lock.json';

/** Return the primary skill-style lockfile path `<dir>/scripts/.lock.json` for a contract path. */
export function resolvePrimaryLockPath(contractPath: string): string {
  const contractDir = dirname(contractPath);

  return join(contractDir, SCRIPTS_DIR_NAME, LOCK_FILE_NAME);
}
