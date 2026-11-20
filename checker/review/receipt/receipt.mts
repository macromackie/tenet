import { dirname, join } from 'node:path';
import { Option, Schema } from 'effect';
import { readInventory } from '../../discovery/index.mts';
import { hashSource, readExisting, readRecord, type RecordData } from '../../storage/index.mts';
import type { Contract, Pin, Request } from '../../types.d.mts';
import { Receipt } from '../../wire.mts';
import { parsePin } from '../pin/index.mts';
import type { Evaluation } from './evidence.mts';

const decodeReceipt = Schema.decodeUnknownOption(Receipt, { onExcessProperty: 'error' });

export type Job = {
  readonly canSpendRequest: () => boolean;
  tools: string;
  contract: Contract;
  request: Request;
};

export type Context = {
  job: Job;
  evaluation: Evaluation;
  pin?: Pin;
  matching: boolean;
  prior: Option.Option<RecordData>;
  receipt: string;
  lock: string;
};

function resolvePaths(contract: Contract) {
  const identity = hashSource(contract.path);
  const receipt = join('agent-checks', `${identity}.json`);
  const directory = dirname(contract.path);
  const lock = join(directory, 'agent-checks.lock.json');

  return { lock, receipt };
}

type ReadReceiptInput = { readonly request: Request; readonly receipt: string };

async function readReceipt({
  request,
  receipt,
}: ReadReceiptInput): Promise<Option.Option<Receipt>> {
  if (request.fresh === true) {
    return Option.none();
  }

  const saved = await readExisting({ path: receipt, root: request.state });

  return decodeReceipt(saved);
}

/** Load bounded sources, the portable pin and the last local observation together. */
export async function readContext(job: Job): Promise<Context> {
  const { contract, request } = job;
  const current = await readInventory({ contract, root: request.root });
  const { receipt, lock } = resolvePaths(contract);
  const saved = await readReceipt({ receipt, request });
  const prior = await readRecord({ path: lock, root: request.root });
  const candidate = parsePin({ contractHash: contract.hash, current, value: prior });

  const evaluation = {
    base: request.base ?? null,
    contract: contract.name,
    current,
    root: request.root,
    saved: Option.getOrUndefined(saved),
    tools: job.tools,
  };

  return { evaluation, job, lock, ...candidate, prior, receipt };
}
