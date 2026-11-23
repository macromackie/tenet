const LEADING_WS_PATTERN = /^(?<indent>[ \t]*)/v;

export type SourceLine = Readonly<{
  end: number;
  eol: string;
  indent: string;
  start: number;
  trimmed: string;
}>;

type ReadLineAtInput = Readonly<{ start: number; text: string }>;
type ReadLineResult = Readonly<{ hasNext: boolean; line: SourceLine; nextStart: number }>;
type RawEndInput = Readonly<{ nl: number; total: number }>;
type LineEndInput = Readonly<{ rawEnd: number; start: number; text: string }>;
type EolSliceInput = Readonly<{ end: number; hasNext: boolean; rawEnd: number; text: string }>;

function readLeadingIndent(line: string): string {
  const match = LEADING_WS_PATTERN.exec(line);

  return match?.groups?.['indent'] ?? '';
}

function readRawEnd({ nl, total }: RawEndInput): number {
  if (nl === -1) {
    return total;
  }

  return nl;
}

function readContentEnd({ rawEnd, start, text }: LineEndInput): number {
  if (rawEnd > start && text[rawEnd - 1] === '\r') {
    return rawEnd - 1;
  }

  return rawEnd;
}

function readEolSlice({ end, hasNext, rawEnd, text }: EolSliceInput): string {
  if (!hasNext) {
    return '';
  }

  return text.slice(end, rawEnd + 1);
}

function readLineAt({ start, text }: ReadLineAtInput): ReadLineResult {
  const nl = text.indexOf('\n', start);
  const hasNext = nl !== -1;
  const rawEnd = readRawEnd({ nl, total: text.length });
  const end = readContentEnd({ rawEnd, start, text });
  const eol = readEolSlice({ end, hasNext, rawEnd, text });
  const content = text.slice(start, end);
  const indent = readLeadingIndent(content);
  const trimmed = content.trim();

  return { hasNext, line: { end, eol, indent, start, trimmed }, nextStart: rawEnd + 1 };
}

/** Split source text into lines with exact spans and indentation metadata. */
export function splitSourceLines(text: string): readonly SourceLine[] {
  const lines: SourceLine[] = [];
  let start = 0;

  while (start <= text.length) {
    const entry = readLineAt({ start, text });

    lines.push(entry.line);

    if (!entry.hasNext) {
      break;
    }

    start = entry.nextStart;
  }

  return lines;
}
