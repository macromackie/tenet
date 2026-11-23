const SAFE_SCRIPT_FILE = /^[a-zA-Z0-9][a-zA-Z0-9._\-]{0,127}$/v;
const LOCK_FILE_NAME = '.lock.json';

/** Validate that a script filename stays strictly inside the contract's `scripts/` directory. */
export function isSafeScriptFile(name: string): boolean {
  if (!SAFE_SCRIPT_FILE.test(name)) {
    return false;
  }

  return !name.includes('..') && name !== LOCK_FILE_NAME;
}
