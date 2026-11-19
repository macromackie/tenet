import type { Inventory, Pin } from '../types.d.mts';
import { hasCheckInputs } from './has-check-inputs.mts';

type IsMatchingInput = {
  readonly current: Inventory;
  readonly contractHash: string;
  readonly pin: Pin;
};

/** Match the governing document and all executable entries before a stored checker can run. */
export function isMatching({ current, contractHash, pin }: IsMatchingInput): boolean {
  if (
    pin.shape !== current.shape ||
    (pin.contractHash !== undefined && pin.contractHash !== contractHash)
  ) {
    return false;
  }

  const { runtimeFiles: files } = current;

  return pin.bundle.checks.every((check) => hasCheckInputs({ check, files }));
}
