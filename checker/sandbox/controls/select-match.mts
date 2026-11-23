import { Option } from 'effect';
import { buildEditError } from './control-diagnostics.mts';

const LINE_DISAMBIGUATION_WINDOW = 15;

export type ControlEdit = Readonly<{
  find: string;
  line?: number;
  occurrence?: number;
  replace: string;
}>;

export type MatchSpan = Readonly<{
  end: number;
  line: number;
  start: number;
}>;

type SelectHintInput<Span extends MatchSpan> = Readonly<{
  edit: ControlEdit;
  matches: readonly Span[];
}>;

type SelectUniqueInput<Span extends MatchSpan> = Readonly<{
  current: string;
  edit: ControlEdit;
  label: string;
  matches: readonly Span[];
}>;

type SelectLineInput<Span extends MatchSpan> = Readonly<{
  matches: readonly Span[];
  targetLine: number;
}>;

function selectByLine<Span extends MatchSpan>({
  matches,
  targetLine,
}: SelectLineInput<Span>): Option.Option<Span> {
  const nearby = matches.filter(
    (match) => Math.abs(match.line - targetLine) <= LINE_DISAMBIGUATION_WINDOW,
  );

  if (nearby.length === 1) {
    const first = nearby.at(0);

    return Option.fromUndefinedOr(first);
  }

  return Option.none();
}

function selectByHint<Span extends MatchSpan>({
  edit,
  matches,
}: SelectHintInput<Span>): Option.Option<Span> {
  if (edit.occurrence !== undefined) {
    const picked = matches.at(edit.occurrence - 1);

    return Option.fromUndefinedOr(picked);
  }

  if (edit.line === undefined) {
    return Option.none();
  }

  return selectByLine({ matches, targetLine: edit.line });
}

/** Pick one unique match or disambiguate via occurrence/line hints, throwing with match lines otherwise. */
export function selectUniqueMatch<Span extends MatchSpan>({
  current,
  edit,
  label,
  matches,
}: SelectUniqueInput<Span>): Span {
  if (matches.length === 1) {
    const sole = matches.at(0);

    if (sole !== undefined) {
      return sole;
    }
  }

  const hinted = selectByHint({ edit, matches });

  if (Option.isSome(hinted)) {
    return hinted.value;
  }

  const matchLines = matches.map((match) => match.line);

  throw buildEditError({ current, find: edit.find, kind: 'ambiguous', label, matchLines });
}
