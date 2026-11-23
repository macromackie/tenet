import { Option, Schema } from 'effect';
import { hashSource } from '../storage/index.mts';
import type { Contract } from '../types.d.mts';

const MAX_CANDIDATES = 5;
const MAX_SLUG_CHARS = 64;
const MAX_MESSAGE_CHARS = 240;
const SLUG_CLEAN_PATTERN = /[^a-z0-9]+/gv;
const TRIM_DASH_PATTERN = /^-+|-+$/gv;

const StringPaths = Schema.Array(Schema.String);
const OptionalPaths = Schema.OptionFromOptionalKey(StringPaths);

const RawCandidate = Schema.Struct({
  message: Schema.String,
  name: Schema.String,
  paths: OptionalPaths,
  rules: Schema.String,
});

type RawCandidate = typeof RawCandidate.Type;

const RawCandidateList = Schema.Array(RawCandidate);
const RawResponse = Schema.Struct({ contracts: RawCandidateList });
const decodeRawResponse = Schema.decodeUnknownOption(RawResponse, { onExcessProperty: 'error' });

export type ParsedCandidate = Readonly<{
  contract: Contract;
  document: string;
  lockPath: string;
  message: string;
  name: string;
  paths: readonly string[];
}>;

type ParseCandidatesInput = Readonly<{
  allowedPaths: readonly string[];
  manifestPaths: readonly string[];
  moduleName: string;
  raw: unknown;
}>;

type FormatDocumentInput = Readonly<{
  message: string;
  name: string;
  paths: readonly string[];
  rules: string;
}>;

type SelectPathsInput = Readonly<{
  fallbackPaths: readonly string[];
  manifestPaths: readonly string[];
  requested: Option.Option<readonly string[]>;
  validPaths: ReadonlySet<string>;
}>;

type BaseNameInput = Readonly<{
  moduleSlug: string;
  rawName: string;
}>;

type UniqueNameInput = Readonly<{
  moduleSlug: string;
  rawName: string;
  seenNames: ReadonlySet<string>;
}>;

type BuildCandidateInput = Readonly<{
  fallbackPaths: readonly string[];
  manifestPaths: readonly string[];
  moduleSlug: string;
  raw: RawCandidate;
  seenNames: ReadonlySet<string>;
  validPaths: ReadonlySet<string>;
}>;

type CollectCandidatesInput = Readonly<{
  allowedPaths: readonly string[];
  items: readonly RawCandidate[];
  manifestPaths: readonly string[];
  moduleSlug: string;
}>;

function formatKebabSlug(value: string): string {
  const lowered = value.trim().toLowerCase();
  const dashed = lowered.replaceAll(SLUG_CLEAN_PATTERN, '-').replaceAll(TRIM_DASH_PATTERN, '');

  return dashed.slice(0, MAX_SLUG_CHARS).replaceAll(TRIM_DASH_PATTERN, '');
}

function escapeYamlSingle(value: string): string {
  return value.replaceAll("'", "''");
}

function formatContractDocument({ message, name, paths, rules }: FormatDocumentInput): string {
  const singleLineMessage = message.replaceAll(/\s+/gv, ' ').trim().slice(0, MAX_MESSAGE_CHARS);
  const pathLines = paths.map((entry) => `  - '${escapeYamlSingle(entry)}'`).join('\n');
  const escapedMessage = escapeYamlSingle(singleLineMessage);

  return [
    '---',
    `name: ${name}`,
    `message: '${escapedMessage}'`,
    'paths:',
    pathLines,
    '---',
    '',
    '## Rules',
    '',
    rules.trim(),
    '',
  ].join('\n');
}

function selectCandidatePaths({
  fallbackPaths,
  manifestPaths,
  requested,
  validPaths,
}: SelectPathsInput): readonly string[] {
  const listed = Option.getOrElse(requested, () => []);
  const filtered = listed.filter((entry) => validPaths.has(entry));

  if (filtered.length > 0) {
    const matchedPaths = new Set<string>([...manifestPaths, ...filtered]);

    return [...matchedPaths].toSorted();
  }

  const fallbackCombinedPaths = new Set<string>([...manifestPaths, ...fallbackPaths]);

  return [...fallbackCombinedPaths].toSorted();
}

function resolveBaseName({ moduleSlug, rawName }: BaseNameInput): string {
  const base = formatKebabSlug(rawName);

  if (base.length > 0) {
    return base;
  }

  return `${moduleSlug}-invariant`;
}

function buildUniqueName({ moduleSlug, rawName, seenNames }: UniqueNameInput): string {
  const candidate = resolveBaseName({ moduleSlug, rawName });

  if (!seenNames.has(candidate)) {
    return candidate;
  }

  return `${moduleSlug}-${candidate}`.slice(0, MAX_SLUG_CHARS).replaceAll(TRIM_DASH_PATTERN, '');
}

function createCandidate({ message, name, paths, rules }: FormatDocumentInput): ParsedCandidate {
  const document = formatContractDocument({ message, name, paths, rules });
  const hash = hashSource(document);
  const contractPath = `audit-pins/${name}/CONTRACT.md`;
  const lockPath = `audit-pins/${name}/scripts/.lock.json`;
  const contract: Contract = { document, hash, name, path: contractPath, paths };

  return { contract, document, lockPath, message, name, paths };
}

function buildParsedCandidate(input: BuildCandidateInput): Option.Option<ParsedCandidate> {
  const message = input.raw.message.replaceAll(/\s+/gv, ' ').trim();
  const rules = input.raw.rules.trim();

  if (message.length === 0 || rules.length === 0) {
    return Option.none();
  }

  const name = buildUniqueName({
    moduleSlug: input.moduleSlug,
    rawName: input.raw.name,
    seenNames: input.seenNames,
  });

  if (name.length === 0 || input.seenNames.has(name)) {
    return Option.none();
  }

  const paths = selectCandidatePaths({
    fallbackPaths: input.fallbackPaths,
    manifestPaths: input.manifestPaths,
    requested: input.raw.paths,
    validPaths: input.validPaths,
  });

  const candidate = createCandidate({ message, name, paths, rules });

  return Option.some(candidate);
}

function collectCandidates({
  allowedPaths,
  items,
  manifestPaths,
  moduleSlug,
}: CollectCandidatesInput): readonly ParsedCandidate[] {
  const validPaths = new Set<string>(allowedPaths);
  const seenNames = new Set<string>();
  const parsed: ParsedCandidate[] = [];

  for (const item of items) {
    const built = buildParsedCandidate({
      fallbackPaths: allowedPaths,
      manifestPaths,
      moduleSlug,
      raw: item,
      seenNames,
      validPaths,
    });

    if (Option.isSome(built)) {
      seenNames.add(built.value.name);
      parsed.push(built.value);
    }
  }

  return parsed;
}

/** Validate, normalize, and format synthesized module contract candidates. */
export function parseAuditCandidates({
  allowedPaths,
  manifestPaths,
  moduleName,
  raw,
}: ParseCandidatesInput): readonly ParsedCandidate[] {
  const decoded = decodeRawResponse(raw);

  if (Option.isNone(decoded)) {
    return [];
  }

  const items = decoded.value.contracts.slice(0, MAX_CANDIDATES);
  const moduleSlug = formatKebabSlug(moduleName);

  return collectCandidates({ allowedPaths, items, manifestPaths, moduleSlug });
}
