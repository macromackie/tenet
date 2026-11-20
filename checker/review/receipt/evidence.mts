import { Result } from 'effect';
import { classifyResult } from '../../classify-result.mts';
import { readBaseInputs } from '../../discovery/index.mts';
import { observe } from '../../sandbox/index.mts';
import { hashSource } from '../../storage/index.mts';
import type { Check, Finding, Inventory, Pin, Receipt } from '../../types.d.mts';
import { buildFinding } from '../build-finding.mts';
import { buildOpenFinding } from '../build-open-finding.mts';

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

  const old = await observe({ check, files: base, qualify: false });

  return !old.passed;
}

type Invocation = {
  check: Check;
  evaluation: Evaluation;
  qualified: boolean;
  reused: boolean;
};

async function observeOne(input: Invocation): Promise<Finding> {
  const { evaluation, check, qualified, reused } = input;

  const context = {
    check: check.id,
    contract: evaluation.contract,
    reused,
    subject: check.subject,
  };

  const result = await observe({
    check,
    files: evaluation.current.runtimeFiles,
    qualify: !qualified,
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

  const message = `${check.promise} (expected ${JSON.stringify(check.expected)}, got ${JSON.stringify(result.value)})`;

  return buildFinding({ ...context, message, outcome: 'failed' });
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
  prequalifiedIds?: ReadonlySet<string>;
};

/** Replay all controls before believing an executable observation on changed inputs. */
export async function evaluate(input: EvaluateInput): Promise<readonly Finding[]> {
  const { pin, evaluation, prequalifiedIds } = input;
  const prior = readQualifications(input);

  const rows = pin.bundle.open.map((reason) =>
    buildOpenFinding({ contract: evaluation.contract, reason }),
  );

  for (const check of pin.bundle.checks) {
    // Each isolated process owns the full sandbox resource allowance.
    const reused = prior.has(check.id);
    const qualified = reused || (prequalifiedIds?.has(check.id) ?? false);
    // oxlint-disable-next-line no-await-in-loop
    const row = await observeCheck({ check, evaluation, qualified, reused });

    rows.push(row);
  }

  return rows;
}
