import { CheckerError } from '../checker-error.mts';

const MAX_PREVIEW_CHARS = 96;
const MAX_NEARBY_HINTS = 3;
const MIN_ANCHOR_CHARS = 6;
const ESCAPED_NEWLINE = String.raw`\n`;

type NearbyInput = Readonly<{
  current: string;
  find: string;
}>;

type AnchorScanInput = Readonly<{
  anchor: string;
  lines: readonly string[];
}>;

type AnchorHitInput = Readonly<{
  index: number;
  line: string;
}>;

type EditErrorInput = Readonly<{
  current: string;
  find: string;
  kind: 'ambiguous' | 'missing';
  label: string;
}>;

function formatPreview(text: string): string {
  const withoutCr = text.replaceAll('\r', '');
  const singleLine = withoutCr.replaceAll('\n', ESCAPED_NEWLINE);

  if (singleLine.length <= MAX_PREVIEW_CHARS) {
    return singleLine;
  }

  return `${singleLine.slice(0, MAX_PREVIEW_CHARS)}...`;
}

function findAnchorLine(find: string): string {
  for (const line of find.split('\n')) {
    const trimmed = line.trim();

    if (trimmed.length >= MIN_ANCHOR_CHARS) {
      return trimmed;
    }
  }

  return find.trim();
}

function formatAnchorHit({ index, line }: AnchorHitInput): string {
  const lineNumber = String(index + 1);
  const trimmedLine = line.trim();
  const preview = formatPreview(trimmedLine);

  return `L${lineNumber}: ${preview}`;
}

function scanAnchorHits({ anchor, lines }: AnchorScanInput): readonly string[] {
  const hits: string[] = [];

  for (const [index, line] of lines.entries()) {
    if (!line.includes(anchor)) {
      continue;
    }

    const hit = formatAnchorHit({ index, line });

    hits.push(hit);

    if (hits.length >= MAX_NEARBY_HINTS) {
      break;
    }
  }

  return hits;
}

function collectNearbyLines({ current, find }: NearbyInput): readonly string[] {
  const anchor = findAnchorLine(find);

  if (anchor.length === 0) {
    return [];
  }

  const lines = current.split('\n');

  return scanAnchorHits({ anchor, lines });
}

type MissingMessageInput = Omit<EditErrorInput, 'kind'>;

function formatMissingMessage({ current, find, label }: MissingMessageInput): string {
  const controlName = JSON.stringify(label);
  const formatted = formatPreview(find);
  const preview = JSON.stringify(formatted);
  const nearby = collectNearbyLines({ current, find });

  if (nearby.length > 0) {
    const joined = nearby.join(' | ');
    const hints = JSON.stringify(joined);

    return `control ${controlName} edit target ${preview} was not found in the subject file; similar lines ${hints}`;
  }

  return `control ${controlName} edit target ${preview} was not found in the subject file`;
}

/** Build an actionable diagnostic error when a control patch target is missing or ambiguous. */
export function buildEditError({ current, find, kind, label }: EditErrorInput): CheckerError {
  if (kind === 'missing') {
    const missingMessage = formatMissingMessage({ current, find, label });

    return new CheckerError({ message: missingMessage });
  }

  const controlName = JSON.stringify(label);
  const formatted = formatPreview(find);
  const preview = JSON.stringify(formatted);
  const ambiguousMessage = `control ${controlName} edit target ${preview} matched multiple locations in the subject file`;

  return new CheckerError({ message: ambiguousMessage });
}
