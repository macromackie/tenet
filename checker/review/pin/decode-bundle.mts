import { Option, Result, Schema } from 'effect';

const MAX_CHECKS = 16;
const MAX_OPEN_LENGTH = 400;
const ELLIPSIS_SUFFIX = '…';
const WHITESPACE_RUN = /\s+/gv;
const RawChecks = Schema.Array(Schema.Unknown);
const OptionalRawChecks = Schema.OptionFromOptionalKey(RawChecks);
const RawOpenItems = Schema.Array(Schema.Unknown);
const OptionalOpenItems = Schema.OptionFromOptionalKey(RawOpenItems);
const RawBundle = Schema.Struct({ checks: OptionalRawChecks, open: OptionalOpenItems });
const decodeRawBundle = Schema.decodeUnknownOption(RawBundle, { onExcessProperty: 'error' });

const OPEN_TEXT_KEYS = ['reason', 'message', 'description', 'summary'] as const;

type DecodedRawBundle = Readonly<{
  open: readonly string[];
  rawChecks: readonly unknown[];
}>;

type BundleProblemInput = Readonly<{
  open: readonly string[];
  rawChecks: readonly unknown[];
}>;

function buildBundleRecord(entries: ReadonlyMap<string, unknown>): Readonly<Record<string, unknown>> {
  const hasChecks = entries.has('checks');
  const hasOpen = entries.has('open');

  if (hasChecks && hasOpen) {
    return { checks: entries.get('checks'), open: entries.get('open') };
  }

  if (hasChecks) {
    return { checks: entries.get('checks') };
  }

  if (hasOpen) {
    return { open: entries.get('open') };
  }

  return {};
}

function readBundleFields(value: unknown): Option.Option<Readonly<Record<string, unknown>>> {
  if (value === null || typeof value !== 'object' || Array.isArray(value)) {
    return Option.none();
  }

  const pairs = Object.entries(value);
  const entries = new Map<string, unknown>(pairs);
  const record = buildBundleRecord(entries);

  return Option.some(record);
}

function readOpenObjectText(fields: ReadonlyMap<string, unknown>): Option.Option<string> {
  for (const key of OPEN_TEXT_KEYS) {
    const candidate = fields.get(key);

    if (typeof candidate === 'string') {
      return Option.some(candidate);
    }
  }

  return Option.none();
}

function extractOpenText(item: unknown): Option.Option<string> {
  if (typeof item === 'string') {
    return Option.some(item);
  }

  if (item === null || typeof item !== 'object' || Array.isArray(item)) {
    return Option.none();
  }

  const pairs = Object.entries(item);
  const fields = new Map<string, unknown>(pairs);

  return readOpenObjectText(fields);
}

function normalizeOpenItem(item: unknown): Option.Option<string> {
  const raw = extractOpenText(item);

  if (Option.isNone(raw)) {
    return Option.none();
  }

  const collapsed = raw.value.replaceAll(WHITESPACE_RUN, ' ').trim();

  if (collapsed.length === 0) {
    return Option.none();
  }

  if (collapsed.length <= MAX_OPEN_LENGTH) {
    return Option.some(collapsed);
  }

  const truncated = collapsed.slice(0, MAX_OPEN_LENGTH - ELLIPSIS_SUFFIX.length).trimEnd();

  return Option.some(`${truncated}${ELLIPSIS_SUFFIX}`);
}

function normalizeOpenItems(items: readonly unknown[]): readonly string[] {
  const normalized: string[] = [];

  for (const item of items) {
    const text = normalizeOpenItem(item);

    if (Option.isSome(text)) {
      normalized.push(text.value);
    }
  }

  return normalized;
}

function findBundleProblem({ open, rawChecks }: BundleProblemInput): Option.Option<string> {
  if (rawChecks.length > MAX_CHECKS || open.length > MAX_CHECKS) {
    return Option.some('checker plan exceeds the number of allowed checks');
  }

  if (rawChecks.length === 0 && open.length === 0) {
    return Option.some('checker plan returned neither checks nor open items');
  }

  return Option.none();
}

function describeInvalidBundle(value: unknown): string {
  if (value === null || typeof value !== 'object' || Array.isArray(value)) {
    return 'the checker plan must be a JSON object with checks and open arrays';
  }

  const keyList = Object.keys(value);
  const keys = JSON.stringify(keyList);

  return `the checker plan has invalid top-level fields ${keys}; return checks and open arrays where open contains strings`;
}

function validateDecodedBundle(bundle: BundleProblemInput): Result.Result<DecodedRawBundle, string> {
  const problem = findBundleProblem(bundle);

  if (Option.isSome(problem)) {
    return Result.fail(problem.value);
  }

  return Result.succeed(bundle);
}

/** Decode, normalize, and bound a raw planner response before validating individual checks. */
export function decodePlannerBundle(value: unknown): Result.Result<DecodedRawBundle, string> {
  const projected = readBundleFields(value);
  const decoded = Option.flatMap(projected, (fields) => decodeRawBundle(fields));

  if (Option.isNone(decoded)) {
    const message = describeInvalidBundle(value);

    return Result.fail(message);
  }

  const rawChecks = Option.getOrElse(decoded.value.checks, () => []);
  const rawOpen = Option.getOrElse(decoded.value.open, () => []);
  const open = normalizeOpenItems(rawOpen);

  return validateDecodedBundle({ open, rawChecks });
}
