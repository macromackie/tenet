import type { Check, Inventory } from '../../types.d.mts';

type HasCheckInputsInput = { readonly check: Check; readonly files: Inventory['runtimeFiles'] };

/** Determine whether this recipe's observed subject and repository-native test are in scope. */
export function hasCheckInputs({ check, files }: HasCheckInputsInput): boolean {
  return files.has(check.subject) && (check.testFile === undefined || files.has(check.testFile));
}
