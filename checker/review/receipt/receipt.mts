import { join } from 'node:path';
import { Option, Schema } from 'effect';
import { readInventory } from '../../discovery/index.mts';
import {
  hashSource,
  readExisting,
  readPinScripts,
  readRecord,
  type RecordData,
  resolveLegacyLockPath,
  resolvePrimaryLockPath,
} from '../../storage/index.mts';
import type { Contract, Pin, Request } from '../../types.d.mts';
import { Pin as PinSchema, Receipt } from '../../wire.mts';
import { parsePin } from '../pin/index.mts';
import type { Evaluation } from './evidence.mts';

const decodePin = Schema.decodeUnknownOption(PinSchema, { onExcessProperty: 'error' });
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
  handEditedIds: ReadonlySet<string>;
  legacyLock: boolean;
  pin?: Pin;
  matching: boolean;
  prior: Option.Option<RecordData>;
  receipt: string;
  lock: string;
};

type ReadPriorInput = Readonly<{
  legacyLock: string;
  lock: string;
  pinRoot: string;
}>;

type PriorRecord = Readonly<{
  legacyLock: boolean;
  lockPath: string;
  prior: Option.Option<RecordData>;
}>;

type ReadPriorScriptsInput = Readonly<{
  lockPath: string;
  pinRoot: string;
  prior: Option.Option<RecordData>;
}>;

type PriorScripts = Readonly<{
  handEditedIds: ReadonlySet<string>;
  hydrated: Option.Option<RecordData>;
}>;

function resolvePaths(contract: Contract) {
  const identity = hashSource(contract.path);
  const receipt = join('agent-checks', `${identity}.json`);
  const lock = resolvePrimaryLockPath(contract.path);
  const legacyLock = resolveLegacyLockPath(contract.path);

  return { legacyLock, lock, receipt };
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

function readPinRoot(request: Request): string {
  if (request.pinRoot !== undefined) {
    return request.pinRoot;
  }

  return request.root;
}

async function readPriorRecord({ legacyLock, lock, pinRoot }: ReadPriorInput): Promise<PriorRecord> {
  const primary = await readRecord({ path: lock, root: pinRoot });

  if (Option.isSome(primary)) {
    return { legacyLock: false, lockPath: lock, prior: primary };
  }

  const legacy = await readRecord({ path: legacyLock, root: pinRoot });

  return { legacyLock: Option.isSome(legacy), lockPath: legacyLock, prior: legacy };
}

async function readPriorScripts({
  lockPath,
  pinRoot,
  prior,
}: ReadPriorScriptsInput): Promise<PriorScripts> {
  if (Option.isNone(prior)) {
    return { handEditedIds: new Set<string>(), hydrated: prior };
  }

  const decoded = decodePin(prior.value.json);

  if (Option.isNone(decoded)) {
    return { handEditedIds: new Set<string>(), hydrated: prior };
  }

  const { handEditedIds, pin } = await readPinScripts({
    lockPath,
    pin: decoded.value,
    root: pinRoot,
  });

  return {
    handEditedIds,
    hydrated: Option.some({ json: pin, text: prior.value.text }),
  };
}

/** Load bounded sources, the portable pin and the last local observation together. */
export async function readContext(job: Job): Promise<Context> {
  const { contract, request } = job;
  const current = await readInventory({ contract, root: request.root });
  const { legacyLock: legacyLockPath, lock, receipt } = resolvePaths(contract);
  const saved = await readReceipt({ receipt, request });
  const pinRoot = readPinRoot(request);
  const priorRecord = await readPriorRecord({ legacyLock: legacyLockPath, lock, pinRoot });

  const { handEditedIds, hydrated } = await readPriorScripts({
    lockPath: priorRecord.lockPath,
    pinRoot,
    prior: priorRecord.prior,
  });

  const candidate = parsePin({ contractHash: contract.hash, current, value: hydrated });

  const evaluation = {
    base: request.base ?? null,
    contract: contract.name,
    current,
    root: request.root,
    saved: Option.getOrUndefined(saved),
    tools: job.tools,
  };

  return {
    evaluation,
    handEditedIds,
    job,
    legacyLock: priorRecord.legacyLock,
    lock,
    ...candidate,
    prior: priorRecord.prior,
    receipt,
  };
}
