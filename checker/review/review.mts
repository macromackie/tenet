import { realpath } from 'node:fs/promises';
import { runnerVersion } from '../sandbox/index.mts';
import { resolveRoot } from '../storage/index.mts';
import type { Finding, Request } from '../types.d.mts';
import { processContract } from './process-contract.mts';

function createBudget(maxGenerations: number): () => boolean {
  let remaining = maxGenerations;

  return () => {
    if (remaining <= 0) {
      return false;
    }

    remaining -= 1;

    return true;
  };
}

async function normalizeRequest(input: Request): Promise<Request> {
  const root = await realpath(input.root);
  const state = await resolveRoot(input.state);

  if (input.pinRoot === undefined) {
    return { ...input, root, state };
  }

  const pinRoot = await resolveRoot(input.pinRoot);

  return { ...input, pinRoot, root, state };
}

/** Assess each contract against a shared provider request allowance. */
export async function run(input: Request): Promise<readonly Finding[]> {
  const request = await normalizeRequest(input);
  const rows: Finding[] = [];
  const tools = await runnerVersion();
  const canSpendRequest = createBudget(input.maxGenerations);

  for (const contract of request.contracts) {
    // Allocation of the next provider request depends on remaining budget.
    // oxlint-disable-next-line no-await-in-loop
    const findings = await processContract({ canSpendRequest, contract, request, tools });

    rows.push(...findings);
  }

  return rows;
}
