import { readFile } from 'node:fs/promises';
import type { Contract, Finding, Inventory } from '../types.d.mts';
import { runFxJson } from './fx-json.mts';
import { resolveWorkerAsset } from './resolve-asset.mts';

export type RepairFeedback = Readonly<{
  errors: readonly string[];
  previous: string;
}>;

type GenerateInput = {
  readonly contract: Contract;
  readonly inventory: Inventory;
  readonly onRequest: (cost: Finding['cost_usd']) => void;
  readonly repair?: RepairFeedback;
};

function encodeInput(request: GenerateInput): string {
  const { contract, inventory, repair } = request;
  // ast-grep-ignore: object-from-entries-typescript -- The fx JSON prompt names each file by its project path.
  const files = Object.fromEntries(inventory.files);

  const payload = {
    applicability_hints: inventory.decisions,
    contract: contract.document,
    files,
    mechanical: contract.mechanical,
    previously_checked: contract.checks,
    reference_samples: inventory.assets,
  };

  if (repair === undefined) {
    return JSON.stringify(payload);
  }

  return JSON.stringify({
    ...payload,
    previous_attempt: repair.previous,
    qualification_errors: repair.errors,
  });
}

/** Generate observations for the current repository and charge at most one provider request. */
export async function generate(request: GenerateInput): Promise<unknown> {
  const promptUrl = await resolveWorkerAsset('checker-prompt.md');
  const instructions = await readFile(promptUrl, 'utf-8');
  const source = encodeInput(request);

  return await runFxJson({ instructions, onRequest: request.onRequest, source });
}
