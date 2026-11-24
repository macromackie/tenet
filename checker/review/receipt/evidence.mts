import { Option, Result } from 'effect';
import { classifyResult } from '../../classify-result.mts';
import { readBaseInputs } from '../../discovery/index.mts';
import { observe } from '../../sandbox/index.mts';
import { hashSource } from '../../storage/index.mts';
import type { Check, Finding, Inventory, Pin, Receipt, WitnessRepro } from '../../types.d.mts';
import { buildFinding } from '../build-finding.mts';
import { buildOpenFinding } from '../build-open-finding.mts';

const MAX_REPRO_CODE_CHARS = 4_000;
const MAX_REPRO_VALUE_CHARS = 512;

export type Evaluation = {
  base: Receipt['base'];
  contract: string;
  current: Inventory;
  root: string;
  saved?: Receipt;
  tools: string;
};

type IsFailingAtBaseInput = {
  check: Check;
  evaluation: Evaluation;
};

async function isFailingAtBase({ evaluation, check }: IsFailingAtBaseInput): Promise<boolean> {
  if (evaluation.base === null) {
    return false;
  }

  const base = await readBaseInputs({
    base: evaluation.base,
    files: evaluation.current.runtimeFiles,
    root: evaluation.root,
  });

  if (!base.has(check.subject)) {
    return false;
  }

  const old = await observe({ check, files: base, projectRoot: evaluation.root, qualify: false });

  return !old.passed;
}

type Invocation = {
  check: Check;
  evaluation: Evaluation;
  pinReused: boolean;
  qualified: boolean;
  reused: boolean;
};

type BuildReproInput = Readonly<{
  check: Check;
  observedValue: unknown;
}>;

function encodeBoundedJson(value: unknown): string {
  const encoded = JSON.stringify(value);

  return encoded.slice(0, MAX_REPRO_VALUE_CHARS);
}

function buildWitnessRepro({ check, observedValue }: BuildReproInput): Option.Option<WitnessRepro> {
  const code = check.test?.trim() ?? '';

  if (code.length === 0) {
    return Option.none();
  }

  const boundedCode = code.slice(0, MAX_REPRO_CODE_CHARS);
  const expected = encodeBoundedJson(check.expected);
  const observed = encodeBoundedJson(observedValue);

  return Option.some({
    code: boundedCode,
    expected,
    observed,
    runtime: check.runtime,
  });
}

type FailedObservationInput = Readonly<{
  check: Check;
  context: Readonly<{
    check: string;
    contract: string;
    pin_reused: boolean;
    reused: boolean;
    subject: string;
  }>;
  observedValue: unknown;
}>;

function buildFailedObservation({
  check,
  context,
  observedValue,
}: FailedObservationInput): Finding {
  const expectedText = JSON.stringify(check.expected);
  const observedText = JSON.stringify(observedValue);
  const message = `${check.promise} (expected ${expectedText}, got ${observedText})`;
  const reproOption = buildWitnessRepro({ check, observedValue });
  const repro = Option.getOrUndefined(reproOption);

  return buildFinding({ ...context, message, outcome: 'failed', repro });
}

function isMissingControl(value?: string): boolean {
  return value === undefined || value.length === 0;
}

function isSlimPinCheck(check: Check): boolean {
  return [check.good, check.defect, check.refactor].every((value) => isMissingControl(value));
}

async function observeOne(input: Invocation): Promise<Finding> {
  const { evaluation, check, pinReused, qualified, reused } = input;

  const context = {
    check: check.id,
    contract: evaluation.contract,
    pin_reused: pinReused,
    reused,
    subject: check.subject,
  };

  const shouldQualify = !qualified && !isSlimPinCheck(check);

  const result = await observe({
    check,
    files: evaluation.current.runtimeFiles,
    projectRoot: evaluation.root,
    qualify: shouldQualify,
  });

  if (result.passed) {
    return buildFinding({ ...context, message: check.promise, outcome: 'passed' });
  }

  if (await isFailingAtBase(input)) {
    return buildFinding({
      ...context,
      message:
        'Also fails on the PR base. Review whether the change worsens the existing behavior.',
      outcome: 'unresolved',
    });
  }

  return buildFailedObservation({ check, context, observedValue: result.value });
}

function readErrorMessage(error: unknown): string {
  if (error instanceof Error) {
    return error.message;
  }

  return 'The observation is unavailable.';
}

async function observeCheck(input: Invocation): Promise<Finding> {
  async function runObservation() {
    return await observeOne(input);
  }

  const result = await classifyResult({
    message: 'observation unavailable',
    run: runObservation,
  });

  if (Result.isSuccess(result)) {
    return result.success;
  }

  const message = readErrorMessage(result.failure.cause);

  return buildFinding({
    check: input.check.id,
    contract: input.evaluation.contract,
    message,
    outcome: 'unresolved',
    pin_reused: input.pinReused,
    subject: input.check.subject,
  });
}

type ReadQualificationsInput = { readonly evaluation: Evaluation; readonly pin: Pin };

function readQualifications({ pin, evaluation }: ReadQualificationsInput): Set<string> {
  const { saved, current, tools } = evaluation;
  const serializedPin = JSON.stringify(pin);

  if (
    saved?.signature !== hashSource(serializedPin) ||
    saved.tools !== tools ||
    saved.observed !== current.observed
  ) {
    return new Set();
  }

  return new Set(saved.qualifications);
}

type EvaluateInput = {
  evaluation: Evaluation;
  pin: Pin;
  pinReused?: boolean;
  prequalifiedIds?: ReadonlySet<string>;
};

/** Replay all controls before believing an executable observation on changed inputs. */
export async function evaluate(input: EvaluateInput): Promise<readonly Finding[]> {
  const { pin, evaluation, pinReused = false, prequalifiedIds } = input;
  const prior = readQualifications(input);

  const rows = pin.bundle.open.map((reason) =>
    buildOpenFinding({ contract: evaluation.contract, reason }),
  );

  for (const check of pin.bundle.checks) {
    // Each isolated process owns the full sandbox resource allowance.
    const reused = prior.has(check.id);
    const qualified = reused || (prequalifiedIds?.has(check.id) ?? false);
    // oxlint-disable-next-line no-await-in-loop
    const row = await observeCheck({ check, evaluation, pinReused, qualified, reused });

    rows.push(row);
  }

  return rows;
}
