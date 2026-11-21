import { basename, extname } from 'node:path';
import type { Contract } from '../types.d.mts';
import { MANIFESTS } from './manifests.mts';

const MIN_TOKEN_LENGTH = 3;
const EXACT_PATH_SCORE = 100;
const STEM_TOKEN_SCORE = 10;
const MAX_RANK_SCORE = 9_999;
const SCORE_PAD_WIDTH = 4;
const BACKTICK_PATTERN = /`(?<token>[^`\n]{2,120})`/gv;
const CAMEL_BOUNDARY = /(?<prev>[a-z0-9])(?<next>[A-Z])/gv;
const NON_WORD_BOUNDARY = /[^a-z0-9]+/gv;

const enumRank = {
  contractMatch: 2,
  failure: 1,
  generatedOrCompat: 5,
  manifest: 0,
  otherPublic: 4,
  privateOrTest: 6,
  publicSource: 3,
  unrelated: 7,
} as const;

const GENERATED_SEGMENTS: ReadonlySet<string> = new Set([
  '_future',
  'compat',
  'gen',
  'generated',
  'third_party',
  'vendor',
]);

const STOP_WORDS: ReadonlySet<string> = new Set([
  'and',
  'any',
  'check',
  'checks',
  'code',
  'contract',
  'error',
  'errors',
  'false',
  'file',
  'files',
  'for',
  'from',
  'function',
  'index',
  'init',
  'input',
  'inputs',
  'lib',
  'line',
  'lines',
  'method',
  'mod',
  'module',
  'name',
  'none',
  'not',
  'null',
  'output',
  'outputs',
  'path',
  'paths',
  'public',
  'return',
  'returns',
  'rule',
  'rules',
  'src',
  'test',
  'tests',
  'the',
  'true',
  'type',
  'value',
  'values',
  'with',
]);

type ContractTerms = Readonly<{
  exactPaths: ReadonlySet<string>;
  tokens: ReadonlySet<string>;
}>;

type RankPathInput = Readonly<{
  decision?: string;
  matchScore: number;
  path: string;
}>;

type CountHitsInput = Readonly<{
  candidates: readonly string[];
  tokens: ReadonlySet<string>;
}>;

type ScoreMatchInput = Readonly<{
  path: string;
  terms: ContractTerms;
}>;

type RankUnclassifiedInput = Readonly<{
  matchScore: number;
  path: string;
}>;

type SortKeyInput = Readonly<{
  group: number;
  matchScore: number;
  path: string;
}>;

function splitIdentifierTokens(text: string): readonly string[] {
  const spaced = text.replaceAll(CAMEL_BOUNDARY, '$<prev> $<next>').toLowerCase();
  const parts = spaced.split(NON_WORD_BOUNDARY);

  return parts.filter((part) => part.length >= MIN_TOKEN_LENGTH && !STOP_WORDS.has(part));
}

function readBacktickValues(document: string): readonly string[] {
  const values: string[] = [];

  for (const match of document.matchAll(BACKTICK_PATTERN)) {
    const raw = match.groups?.['token']?.trim();

    if (raw !== undefined && raw.length > 0) {
      values.push(raw);
    }
  }

  return values;
}

function readExactPaths(backticks: readonly string[]): ReadonlySet<string> {
  const exactPaths = new Set<string>();

  for (const raw of backticks) {
    if (raw.includes('/') && !raw.includes(' ')) {
      const cleaned = raw.replace(/^\.\//v, '');

      exactPaths.add(cleaned);
    }
  }

  return exactPaths;
}

function readContractTerms(contract: Contract): ContractTerms {
  const backticks = readBacktickValues(contract.document);
  const exactPaths = readExactPaths(backticks);
  const nameTokens = splitIdentifierTokens(contract.name);
  const tokens = new Set<string>(nameTokens);

  for (const raw of backticks) {
    for (const token of splitIdentifierTokens(raw)) {
      tokens.add(token);
    }
  }

  return { exactPaths, tokens };
}

function isPrivateModule(path: string): boolean {
  const name = basename(path);

  return name.startsWith('_') && name !== '__init__.py';
}

function isTestPath(path: string): boolean {
  const segments = path.split('/');

  return segments.includes('test') || segments.includes('tests') || path.endsWith('_test.go');
}

function isGeneratedOrCompatPath(path: string): boolean {
  const segments = path.split('/');

  return segments.some((segment) => GENERATED_SEGMENTS.has(segment));
}

function readPathStem(path: string): string {
  const name = basename(path);
  const extension = extname(name);

  if (extension.length === 0) {
    return name;
  }

  return name.slice(0, -extension.length);
}

function countTokenHits({ candidates, tokens }: CountHitsInput): number {
  let hits = 0;

  for (const candidate of candidates) {
    if (tokens.has(candidate)) {
      hits += 1;
    }
  }

  return hits;
}

function scoreContractMatch({ path, terms }: ScoreMatchInput): number {
  if (terms.exactPaths.has(path)) {
    return EXACT_PATH_SCORE;
  }

  const stem = readPathStem(path);
  const stemCandidates = splitIdentifierTokens(stem);
  const stemHits = countTokenHits({ candidates: stemCandidates, tokens: terms.tokens });
  const pathCandidates = splitIdentifierTokens(path);
  const pathHits = countTokenHits({ candidates: pathCandidates, tokens: terms.tokens });

  return stemHits * STEM_TOKEN_SCORE + pathHits;
}

function rankUnclassifiedPath({ matchScore, path }: RankUnclassifiedInput): number {
  if (isPrivateModule(path) || isTestPath(path)) {
    return enumRank.privateOrTest;
  }

  if (isGeneratedOrCompatPath(path)) {
    return enumRank.generatedOrCompat;
  }

  if (matchScore > 0) {
    return enumRank.contractMatch;
  }

  if (path.startsWith('src/') || path.startsWith('lib/')) {
    return enumRank.publicSource;
  }

  return enumRank.otherPublic;
}

function rankPath({ decision, matchScore, path }: RankPathInput): number {
  const name = basename(path);

  if (MANIFESTS.has(name)) {
    return enumRank.manifest;
  }

  if (decision === 'fail') {
    return enumRank.failure;
  }

  if (decision === 'not_applicable') {
    return enumRank.unrelated;
  }

  return rankUnclassifiedPath({ matchScore, path });
}

function buildSortKey({ group, matchScore, path }: SortKeyInput): string {
  const boundedScore = Math.min(matchScore, MAX_RANK_SCORE);
  const inverted = String(MAX_RANK_SCORE - boundedScore).padStart(SCORE_PAD_WIDTH, '0');

  return `${String(group)}:${inverted}:${path}`;
}

function readOrderedValues(rankedPaths: ReadonlyMap<string, string>): readonly string[] {
  const ordered: string[] = [];

  for (const key of [...rankedPaths.keys()].toSorted()) {
    const entry = rankedPaths.get(key);

    if (entry !== undefined) {
      ordered.push(entry);
    }
  }

  return ordered;
}

/** Order governed paths so manifests, flagged files, and contract-referenced modules fit ahead of generated or internal files. */
export function listRankedPaths(contract: Contract): readonly string[] {
  const terms = readContractTerms(contract);
  const rankedPaths = new Map<string, string>();

  for (const path of contract.paths.toSorted()) {
    const matchScore = scoreContractMatch({ path, terms });
    const group = rankPath({ decision: contract.decisions?.[path], matchScore, path });
    const key = buildSortKey({ group, matchScore, path });

    rankedPaths.set(key, path);
  }

  return readOrderedValues(rankedPaths);
}
