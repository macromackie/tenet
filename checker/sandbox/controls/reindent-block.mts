import { Option } from 'effect';
import { buildEditError } from './control-diagnostics.mts';
import { type ControlEdit, type MatchSpan, selectUniqueMatch } from './select-match.mts';
import { type SourceLine, splitSourceLines } from './split-source-lines.mts';

const LEADING_WS_PATTERN = /^(?<indent>[ \t]*)/v;
const CRLF_LENGTH = 2;

type BlockMatch = MatchSpan & Readonly<{ findIndent: string; sourceIndent: string }>;
type NormalizedEdit = Readonly<{ find: string; replace: string }>;
type AdjustIndentInput = Readonly<{ findIndent: string; lineIndent: string; sourceIndent: string }>;
type FormatLineInput = Readonly<{ findIndent: string; line: string; sourceIndent: string }>;

type FormatBlockInput = Readonly<{
  eol: string; findIndent: string; replaceText: string; sourceIndent: string;
}>;

type LinesMatchInput = Readonly<{
  findLines: readonly string[]; sourceLines: readonly SourceLine[]; startIndex: number;
}>;

type BlockAtInput = Readonly<{
  consumeTrailingEol: boolean; findLines: readonly string[];
  firstNonEmptyOffset: number; sourceLines: readonly SourceLine[]; startIndex: number;
}>;

type CollectBlocksInput = Readonly<{
  consumeTrailingEol: boolean; findLines: readonly string[]; sourceLines: readonly SourceLine[];
}>;

type BuildBlockInput = Readonly<{
  anchorFind: string;
  anchorSource: SourceLine;
  consumeTrailingEol: boolean;
  first: SourceLine;
  last: SourceLine;
  startIndex: number;
}>;

type ApplyTrimmedInput = Readonly<{
  current: string;
  edit: ControlEdit;
  label: string;
  normalized: NormalizedEdit;
}>;

type ReplaceBlockInput = Readonly<{
  current: string;
  match: BlockMatch;
  normalized: NormalizedEdit;
}>;

function readLeadingIndent(line: string): string {
  const match = LEADING_WS_PATTERN.exec(line);

  return match?.groups?.['indent'] ?? '';
}

function readFileEol(text: string): string {
  if (text.includes('\r\n')) {
    return '\r\n';
  }

  return '\n';
}

function stripOneTrailingEol(text: string): string {
  if (text.endsWith('\r\n')) {
    return text.slice(0, -CRLF_LENGTH);
  }

  if (text.endsWith('\n')) {
    return text.slice(0, -1);
  }

  return text;
}

function hasTrimmedBlockAt({ findLines, sourceLines, startIndex }: LinesMatchInput): boolean {
  for (let offset = 0; offset < findLines.length; offset += 1) {
    const sourceLine = sourceLines.at(startIndex + offset);
    const findLine = findLines.at(offset);

    if (sourceLine === undefined || findLine === undefined) {
      return false;
    }

    if (sourceLine.trimmed !== findLine.trim()) {
      return false;
    }
  }

  return true;
}

function buildBlockMatch({
  anchorFind,
  anchorSource,
  consumeTrailingEol,
  first,
  last,
  startIndex,
}: BuildBlockInput): BlockMatch {
  const findIndent = readLeadingIndent(anchorFind);

  if (consumeTrailingEol) {
    return {
      end: last.end + last.eol.length,
      findIndent,
      line: startIndex + 1,
      sourceIndent: anchorSource.indent,
      start: first.start,
    };
  }

  return {
    end: last.end,
    findIndent,
    line: startIndex + 1,
    sourceIndent: anchorSource.indent,
    start: first.start,
  };
}

type BlockAnchors = Readonly<{
  anchorFind: string;
  anchorSource: SourceLine;
  first: SourceLine;
  last: SourceLine;
}>;

type BlockAnchorsInput = Omit<BlockAtInput, 'consumeTrailingEol'>;

function readBlockAnchors({
  findLines,
  firstNonEmptyOffset,
  sourceLines,
  startIndex,
}: BlockAnchorsInput): Option.Option<BlockAnchors> {
  const rawFind = findLines.at(firstNonEmptyOffset);
  const rawAnchor = sourceLines.at(startIndex + firstNonEmptyOffset);
  const rawFirst = sourceLines.at(startIndex);
  const rawLast = sourceLines.at(startIndex + findLines.length - 1);
  const anchorFind = Option.fromUndefinedOr(rawFind);
  const anchorSource = Option.fromUndefinedOr(rawAnchor);
  const first = Option.fromUndefinedOr(rawFirst);
  const last = Option.fromUndefinedOr(rawLast);

  return Option.all({ anchorFind, anchorSource, first, last });
}

function matchBlockAt({
  consumeTrailingEol,
  findLines,
  firstNonEmptyOffset,
  sourceLines,
  startIndex,
}: BlockAtInput): Option.Option<BlockMatch> {
  if (!hasTrimmedBlockAt({ findLines, sourceLines, startIndex })) {
    return Option.none();
  }

  const anchors = readBlockAnchors({ findLines, firstNonEmptyOffset, sourceLines, startIndex });

  return Option.map(anchors, (found) =>
    buildBlockMatch({ ...found, consumeTrailingEol, startIndex }),
  );
}

function collectBlockMatches({
  consumeTrailingEol,
  findLines,
  sourceLines,
}: CollectBlocksInput): readonly BlockMatch[] {
  const firstNonEmptyOffset = findLines.findIndex((line) => line.trim().length > 0);

  if (firstNonEmptyOffset === -1 || findLines.length > sourceLines.length) {
    return [];
  }

  const matches: BlockMatch[] = [];
  const lastStart = sourceLines.length - findLines.length;

  for (let startIndex = 0; startIndex <= lastStart; startIndex += 1) {
    const hit = matchBlockAt({
      consumeTrailingEol,
      findLines,
      firstNonEmptyOffset,
      sourceLines,
      startIndex,
    });

    if (Option.isSome(hit)) {
      matches.push(hit.value);
    }
  }

  return matches;
}

function computeAdjustedIndent({ findIndent, lineIndent, sourceIndent }: AdjustIndentInput): string {
  if (lineIndent.startsWith(findIndent)) {
    const relative = lineIndent.slice(findIndent.length);

    return `${sourceIndent}${relative}`;
  }

  if (findIndent.startsWith(lineIndent)) {
    const dedent = findIndent.length - lineIndent.length;
    const keepLength = Math.max(0, sourceIndent.length - dedent);

    return sourceIndent.slice(0, keepLength);
  }

  return sourceIndent;
}

function formatIndentedLine({ findIndent, line, sourceIndent }: FormatLineInput): string {
  if (line.trim().length === 0) {
    return '';
  }

  const lineIndent = readLeadingIndent(line);
  const rest = line.slice(lineIndent.length);
  const nextIndent = computeAdjustedIndent({ findIndent, lineIndent, sourceIndent });

  return `${nextIndent}${rest}`;
}

function formatIndentedBlock({
  eol,
  findIndent,
  replaceText,
  sourceIndent,
}: FormatBlockInput): string {
  if (replaceText.length === 0) {
    return '';
  }

  const normalized = replaceText.replaceAll('\r\n', '\n');
  const rawLines = normalized.split('\n');
  const adjusted = rawLines.map((line) => formatIndentedLine({ findIndent, line, sourceIndent }));

  return adjusted.join(eol);
}

function readReplaceBody(normalized: NormalizedEdit): string {
  if (normalized.find.endsWith('\n')) {
    return stripOneTrailingEol(normalized.replace);
  }

  return normalized.replace;
}

function replaceBlockMatch({ current, match, normalized }: ReplaceBlockInput): string {
  const replaceText = readReplaceBody(normalized);
  const eol = readFileEol(current);

  const replacement = formatIndentedBlock({
    eol,
    findIndent: match.findIndent,
    replaceText,
    sourceIndent: match.sourceIndent,
  });

  const prefix = current.slice(0, match.start);
  const suffix = current.slice(match.end);

  return `${prefix}${replacement}${suffix}`;
}

/** Apply a find/replace edit by contiguous trimmed-line block matching with indentation delta preservation. */
export function applyTrimmedBlockEdit({
  current,
  edit,
  label,
  normalized,
}: ApplyTrimmedInput): string {
  const stripped = stripOneTrailingEol(normalized.find);
  const findLines = stripped.replaceAll('\r\n', '\n').split('\n');
  const consumeTrailingEol = normalized.find.endsWith('\n') && normalized.replace.length === 0;
  const sourceLines = splitSourceLines(current);
  const matches = collectBlockMatches({ consumeTrailingEol, findLines, sourceLines });

  if (matches.length === 0) {
    throw buildEditError({ current, find: edit.find, kind: 'missing', label });
  }

  const match = selectUniqueMatch({ current, edit, label, matches });

  return replaceBlockMatch({ current, match, normalized });
}
