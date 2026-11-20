import { env } from 'node:process';
import { Option, Result, Schema } from 'effect';
import { classifyResult } from '../classify-result.mts';
import { reportProgress } from '../progress-log.mts';
import type { Contract, Finding } from '../types.d.mts';
import { runFxJson } from './fx-json.mts';

const EXPLAIN_TIMEOUT_MS = 30_000;
const MAX_EXPLANATION_CHARS = 480;
const ASCII_SPACE = 32;
const ASCII_DEL = 127;
const EXPLAINABLE_OUTCOMES: ReadonlySet<Finding['outcome']> = new Set(['failed', 'unresolved']);

const ExplanationMap = Schema.Record(Schema.String, Schema.String);
const ExplanationPayload = Schema.Struct({ explanations: ExplanationMap });

const decodeExplanations = Schema.decodeUnknownOption(ExplanationPayload, {
  onExcessProperty: 'error',
});

const EXPLAIN_INSTRUCTIONS = [
  'You write concise, situation-specific CLI diagnostic messages for Tenet contract checks.',
  'Input JSON contains a contract and a list of failed or unresolved behavior-check findings.',
  'Return JSON: {"explanations": {"<index>": "<1-2 concise plain-English sentences>"}}.',
  'For each failed finding: state clearly what the code did vs what the contract requires, citing the exact observed and expected values from raw_message without dumping raw JSON blobs when a short summary is clearer.',
  'For each unresolved finding: explain the concrete blocker (missing sandbox/runtime, qualification mismatch, PR base failure, or coverage gap) and the single clearest next step.',
  'Never invent files, line numbers, APIs, or behaviors not present in the input. Never contradict the outcome. Plain text only (no markdown headings or code fences).',
].join('\n');

type ExplainFindingsInput = Readonly<{
  canSpendRequest: () => boolean;
  contract: Contract;
  onRequest: (cost: Finding['cost_usd']) => void;
  progressFile?: string;
  rows: readonly Finding[];
}>;

type CandidateEntry = Readonly<{
  check: string;
  index: number;
  outcome: Finding['outcome'];
  raw_message: string;
  subject: Finding['subject'];
}>;

type ApplyInput = Readonly<{
  explanations: Readonly<Record<string, string>>;
  rows: readonly Finding[];
}>;

type UpdateRowInput = Readonly<{
  explanations: Readonly<Record<string, string>>;
  index: number;
  row: Finding;
}>;

function mapPrintableChar(character: string): string {
  const code = character.codePointAt(0) ?? 0;

  if (code < ASCII_SPACE || code === ASCII_DEL) {
    return ' ';
  }

  return character;
}

function cleanText(text: string): string {
  const characters = Array.from(text, (character) => mapPrintableChar(character));
  const collapsed = characters.join('').replaceAll(/\s+/gv, ' ').trim();

  if (collapsed.length <= MAX_EXPLANATION_CHARS) {
    return collapsed;
  }

  return collapsed.slice(0, MAX_EXPLANATION_CHARS);
}

function collectCandidates(rows: readonly Finding[]): readonly CandidateEntry[] {
  const candidates: CandidateEntry[] = [];

  for (const [index, row] of rows.entries()) {
    if (EXPLAINABLE_OUTCOMES.has(row.outcome)) {
      candidates.push({
        check: row.check,
        index,
        outcome: row.outcome,
        raw_message: row.message,
        subject: row.subject,
      });
    }
  }

  return candidates;
}

function hasApiKey(): boolean {
  const key = env.OPENROUTER_API_KEY;

  return key !== undefined && key.length > 0;
}

function updateRow({ explanations, index, row }: UpdateRowInput): Finding {
  const key = String(index);

  if (row.outcome === 'passed' || !Object.hasOwn(explanations, key)) {
    return row;
  }

  const message = cleanText(explanations[key] ?? '');

  if (message.length === 0) {
    return row;
  }

  return { ...row, message };
}

function applyExplanations({ explanations, rows }: ApplyInput): readonly Finding[] {
  const updated: Finding[] = [];

  for (const [index, row] of rows.entries()) {
    const nextRow = updateRow({ explanations, index, row });

    updated.push(nextRow);
  }

  return updated;
}

type ReadExplanationsInput = Readonly<{
  candidates: readonly CandidateEntry[];
  contract: Contract;
  onRequest: (cost: Finding['cost_usd']) => void;
}>;

async function readExplanations({
  candidates,
  contract,
  onRequest,
}: ReadExplanationsInput): Promise<Option.Option<Readonly<Record<string, string>>>> {
  const source = JSON.stringify({
    contract: contract.document,
    contract_name: contract.name,
    findings: candidates,
  });

  async function requestExplanations(): Promise<unknown> {
    return await runFxJson({
      instructions: EXPLAIN_INSTRUCTIONS,
      onRequest,
      source,
      timeoutMs: EXPLAIN_TIMEOUT_MS,
    });
  }

  const classified = await classifyResult({
    message: 'finding explanation unavailable',
    run: requestExplanations,
  });

  if (Result.isFailure(classified)) {
    return Option.none();
  }

  const decoded = decodeExplanations(classified.success);

  return Option.map(decoded, (payload) => payload.explanations);
}

/** Synthesize concise, grounded failure and error diagnostics through fx without changing outcomes. */
export async function explainFindings({
  canSpendRequest,
  contract,
  onRequest,
  progressFile,
  rows,
}: ExplainFindingsInput): Promise<readonly Finding[]> {
  const candidates = collectCandidates(rows);

  if (candidates.length === 0 || !hasApiKey() || !canSpendRequest()) {
    return rows;
  }

  await reportProgress({ contract: contract.name, phase: 'explaining findings', progressFile });

  const explanations = await readExplanations({ candidates, contract, onRequest });

  if (Option.isNone(explanations)) {
    return rows;
  }

  return applyExplanations({ explanations: explanations.value, rows });
}
