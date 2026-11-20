import { CheckerError } from '../checker-error.mts';
import { generate, type RepairFeedback } from '../fx/index.mts';
import { reportProgress } from '../progress-log.mts';
import { assertSandboxAvailable } from '../sandbox/index.mts';
import type { Finding, Pin } from '../types.d.mts';
import { type CandidateEvaluation, qualifyBundle, updatePin } from './pin/index.mts';
import type { Context } from './receipt/index.mts';

const MAX_REPAIR_PREVIEW_CHARS = 32_768;

export type Checked = {
  cache: boolean;
  pin: Pin;
  rows: readonly Finding[];
};

type SelectedPin = Readonly<{
  extraUnresolved: readonly Finding[];
  pin: Pin;
  pinReused: boolean;
  prequalifiedIds: ReadonlySet<string>;
}>;

export type Preparation = {
  ctx: Context;
  onRequest: (cost: Finding['cost_usd']) => void;
};

type GenerationCallInput = Preparation & Readonly<{ repair?: RepairFeedback }>;

type GeneratedAttempt = Readonly<{ evaluated: CandidateEvaluation; raw: unknown }>;

type BuildSelectedInput = Readonly<{ ctx: Context; evaluated: CandidateEvaluation }>;

type RepairIfNeededInput = Readonly<{ first: GeneratedAttempt; input: Preparation }>;

type MergePriorInput = Readonly<{ ctx: Context; generated: SelectedPin }>;

function formatPreview(value: unknown): string {
  const serialized = JSON.stringify(value);

  if (serialized.length <= MAX_REPAIR_PREVIEW_CHARS) {
    return serialized;
  }

  return serialized.slice(0, MAX_REPAIR_PREVIEW_CHARS);
}

async function callModel({ ctx, onRequest, repair }: GenerationCallInput): Promise<unknown> {
  const { contract } = ctx.job;
  const { current } = ctx.evaluation;

  return await generate({ contract, inventory: current, onRequest, repair });
}

function readGenerationPhase(repair?: RepairFeedback): string {
  if (repair === undefined) {
    return 'generating checks';
  }

  return 'repairing checks';
}

async function generateOnce(input: GenerationCallInput): Promise<GeneratedAttempt> {
  const { ctx, repair } = input;
  const phase = readGenerationPhase(repair);

  await reportProgress({
    contract: ctx.job.contract.name,
    phase,
    progressFile: ctx.job.request.progressFile,
  });

  const raw = await callModel(input);

  await reportProgress({
    contract: ctx.job.contract.name,
    phase: 'qualifying controls',
    progressFile: ctx.job.request.progressFile,
  });

  const evaluated = await qualifyBundle({
    contractName: ctx.job.contract.name,
    current: ctx.evaluation.current,
    value: raw,
  });

  return { evaluated, raw };
}

function shouldRepair(evaluated: CandidateEvaluation): boolean {
  return evaluated.qualified.length === 0 && evaluated.errors.length > 0;
}

function buildSelectedPin({ ctx, evaluated }: BuildSelectedInput): SelectedPin {
  const pin: Pin = {
    bundle: { checks: evaluated.qualified, open: evaluated.open },
    contractHash: ctx.job.contract.hash,
    protocol: 1,
    shape: ctx.evaluation.current.shape,
  };

  const qualifiedIds = evaluated.qualified.map((check) => check.id);
  const prequalifiedIds = new Set(qualifiedIds);

  return { extraUnresolved: evaluated.extraUnresolved, pin, pinReused: false, prequalifiedIds };
}

async function runRepairIfNeeded({
  first,
  input,
}: RepairIfNeededInput): Promise<CandidateEvaluation> {
  if (!shouldRepair(first.evaluated) || !input.ctx.job.canSpendRequest()) {
    return first.evaluated;
  }

  const repair: RepairFeedback = {
    errors: first.evaluated.errors,
    previous: formatPreview(first.raw),
  };

  const repaired = await generateOnce({ ...input, repair });

  return repaired.evaluated;
}

async function generateAndQualify(input: Preparation): Promise<SelectedPin> {
  const { ctx } = input;

  if (!ctx.job.canSpendRequest()) {
    throw new CheckerError({ message: 'no saved checker. Generation needs a model request.' });
  }

  const first = await generateOnce(input);
  const evaluated = await runRepairIfNeeded({ first, input });

  return buildSelectedPin({ ctx, evaluated });
}

function mergePriorPin({ ctx, generated }: MergePriorInput): SelectedPin {
  if (ctx.pin === undefined) {
    return generated;
  }

  const pin = updatePin({
    files: ctx.evaluation.current.runtimeFiles,
    next: generated.pin,
    prior: ctx.pin,
  });

  return { ...generated, pin };
}

/** Discover a replacement when the contract or its observed repository surface changes. */
export async function selectPin(input: Preparation): Promise<SelectedPin> {
  const { ctx } = input;

  if (ctx.evaluation.current.files.size === 0) {
    throw new CheckerError({ message: 'no supported source was available for a behavior check.' });
  }

  if (ctx.pin !== undefined && ctx.matching) {
    return { extraUnresolved: [], pin: ctx.pin, pinReused: true, prequalifiedIds: new Set() };
  }

  await assertSandboxAvailable();

  const generated = await generateAndQualify(input);

  return mergePriorPin({ ctx, generated });
}
