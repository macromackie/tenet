import { realpath } from 'node:fs/promises';
import { resolve } from 'node:path';
import { runnerVersion } from '../sandbox/index.mts';
import type { Finding, Request } from '../types.d.mts';
import { processContract } from './process-contract.mts';

/** Assess each contract against a shared provider request allowance. */
export async function run(input: Request): Promise<readonly Finding[]> {
  const request = { ...input, root: await realpath(input.root), state: resolve(input.state) };
  const rows: Finding[] = [];
  const tools = await runnerVersion();
  let remaining = input.maxGenerations;

  function canSpendRequest(): boolean {
    if (remaining <= 0) {
      return false;
    }

    remaining -= 1;

    return true;
  }

  for (const contract of request.contracts) {
    // Allocation of the next provider request depends on remaining budget.
    // oxlint-disable-next-line no-await-in-loop
    const findings = await processContract({ canSpendRequest, contract, request, tools });

    rows.push(...findings);
  }

  return rows;
}
