import { applyTrimmedBlockEdit } from './reindent-block.mts';
import { type ControlEdit, type MatchSpan, selectUniqueMatch } from './select-match.mts';

export type { ControlEdit } from './select-match.mts';

type NormalizedEdit = Readonly<{ find: string; replace: string }>;
type NormalizeInput = Readonly<{ current: string; edit: ControlEdit }>;
type LineNumberInput = Readonly<{ current: string; offset: number }>;
type ExactMatchesInput = Readonly<{ current: string; target: string }>;
type ApplySingleEditInput = Readonly<{ current: string; edit: ControlEdit; label: string }>;

function convertToCrlf(text: string): string {
  const lf = text.replaceAll('\r\n', '\n');

  return lf.replaceAll('\n', '\r\n');
}

function normalizeEditEol({ current, edit }: NormalizeInput): NormalizedEdit {
  const sourceHasCrlf = current.includes('\r\n');

  if (current.includes(edit.find)) {
    if (sourceHasCrlf && edit.replace.includes('\n') && !edit.replace.includes('\r\n')) {
      return { find: edit.find, replace: convertToCrlf(edit.replace) };
    }

    return { find: edit.find, replace: edit.replace };
  }

  if (sourceHasCrlf && edit.find.includes('\n')) {
    return { find: convertToCrlf(edit.find), replace: convertToCrlf(edit.replace) };
  }

  if (!sourceHasCrlf && edit.find.includes('\r\n')) {
    return {
      find: edit.find.replaceAll('\r\n', '\n'),
      replace: edit.replace.replaceAll('\r\n', '\n'),
    };
  }

  return { find: edit.find, replace: edit.replace };
}

function readLineNumberAt({ current, offset }: LineNumberInput): number {
  let line = 1;

  for (let index = 0; index < offset; index += 1) {
    if (current[index] === '\n') {
      line += 1;
    }
  }

  return line;
}

function collectExactMatches({ current, target }: ExactMatchesInput): readonly MatchSpan[] {
  const matches: MatchSpan[] = [];
  let cursor = current.indexOf(target);

  while (cursor !== -1) {
    const line = readLineNumberAt({ current, offset: cursor });

    matches.push({ end: cursor + target.length, line, start: cursor });

    cursor = current.indexOf(target, cursor + 1);
  }

  return matches;
}

/** Apply one control find/replace edit using EOL normalization, exact matching, or trimmed-line fallback. */
export function applySingleEdit({ current, edit, label }: ApplySingleEditInput): string {
  const normalized = normalizeEditEol({ current, edit });
  const exactMatches = collectExactMatches({ current, target: normalized.find });

  if (exactMatches.length > 0) {
    const match = selectUniqueMatch({ current, edit, label, matches: exactMatches });
    const prefix = current.slice(0, match.start);
    const suffix = current.slice(match.end);

    return `${prefix}${normalized.replace}${suffix}`;
  }

  return applyTrimmedBlockEdit({ current, edit, label, normalized });
}
