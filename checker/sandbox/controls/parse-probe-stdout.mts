import { Option, Schema } from 'effect';
import { CheckerError } from '../../checker-error.mts';
import { stripControlChars } from './strip-control-chars.mts';

const TENET_JSON_SENTINEL = '__TENET_JSON__:';
const MAX_TRAILING_LINES = 200;
const MAX_ERROR_PREVIEW_CHARS = 160;

const JsonFromString = Schema.fromJsonString(Schema.Json);
const decodeJson = Schema.decodeUnknownOption(JsonFromString, { onExcessProperty: 'error' });

type TrailingSliceInput = Readonly<{
  lastIndex: number;
  lines: readonly string[];
}>;

type SliceAttemptInput = Readonly<{
  lastIndex: number;
  lines: readonly string[];
  start: number;
}>;

type LastLineInfo = Readonly<{
  index: number;
  text: string;
}>;

function cleanLine(text: string): string {
  const collapsed = stripControlChars(text).replaceAll(/\s+/gv, ' ').trim();

  if (collapsed.length <= MAX_ERROR_PREVIEW_CHARS) {
    return collapsed;
  }

  return `${collapsed.slice(0, MAX_ERROR_PREVIEW_CHARS)}...`;
}

function buildInvalidStdoutError(lastLine: string): CheckerError {
  const sanitized = cleanLine(lastLine);
  const preview = JSON.stringify(sanitized);
  const message = `the isolated test produced non-JSON stdout (last line: ${preview})`;

  return new CheckerError({ message });
}

function findLastNonEmptyLine(lines: readonly string[]): Option.Option<LastLineInfo> {
  for (let index = lines.length - 1; index >= 0; index -= 1) {
    const candidate = lines.at(index)?.trim() ?? '';

    if (candidate.length > 0) {
      return Option.some({ index, text: candidate });
    }
  }

  return Option.none();
}

function parseSentinelLine(lines: readonly string[]): Option.Option<Schema.Json> {
  for (let index = lines.length - 1; index >= 0; index -= 1) {
    const trimmed = lines.at(index)?.trim() ?? '';

    if (trimmed.startsWith(TENET_JSON_SENTINEL)) {
      const payload = trimmed.slice(TENET_JSON_SENTINEL.length).trim();
      const parsed = decodeJson(payload);

      if (Option.isSome(parsed)) {
        return parsed;
      }

      throw buildInvalidStdoutError(trimmed);
    }
  }

  return Option.none();
}

function isBlockStart(line: string): boolean {
  return line.startsWith('{') || line.startsWith('[');
}

function isBlockEnd(line: string): boolean {
  return line.endsWith('}') || line.endsWith(']');
}

function trySliceAt({ lastIndex, lines, start }: SliceAttemptInput): Option.Option<Schema.Json> {
  const candidate = lines.at(start)?.trim() ?? '';

  if (!isBlockStart(candidate)) {
    return Option.none();
  }

  const slice = lines.slice(start, lastIndex + 1).join('\n');

  return decodeJson(slice);
}

function parseTrailingBlock({ lastIndex, lines }: TrailingSliceInput): Option.Option<Schema.Json> {
  const lastText = lines.at(lastIndex)?.trim() ?? '';

  if (!isBlockEnd(lastText)) {
    return Option.none();
  }

  const floor = Math.max(0, lastIndex - MAX_TRAILING_LINES + 1);

  for (let start = lastIndex - 1; start >= floor; start -= 1) {
    const parsed = trySliceAt({ lastIndex, lines, start });

    if (Option.isSome(parsed)) {
      return parsed;
    }
  }

  return Option.none();
}

function parseTailJson({ lastIndex, lines }: TrailingSliceInput): Option.Option<Schema.Json> {
  const lastText = lines.at(lastIndex)?.trim() ?? '';
  const lastParsed = decodeJson(lastText);

  if (Option.isSome(lastParsed)) {
    return lastParsed;
  }

  return parseTrailingBlock({ lastIndex, lines });
}

function parseFromLines(lines: readonly string[]): Schema.Json {
  const sentinelValue = parseSentinelLine(lines);

  if (Option.isSome(sentinelValue)) {
    return sentinelValue.value;
  }

  const lastInfo = findLastNonEmptyLine(lines);

  if (Option.isNone(lastInfo)) {
    throw new CheckerError({ message: 'the isolated test produced empty stdout instead of JSON' });
  }

  const tailValue = parseTailJson({ lastIndex: lastInfo.value.index, lines });

  if (Option.isSome(tailValue)) {
    return tailValue.value;
  }

  throw buildInvalidStdoutError(lastInfo.value.text);
}

/** Parse one JSON observation from probe stdout while tolerating noisy framework log lines. */
export function parseProbeStdout(stdout: string): Schema.Json {
  const trimmed = stdout.trim();

  if (trimmed.length === 0) {
    throw new CheckerError({ message: 'the isolated test produced empty stdout instead of JSON' });
  }

  const direct = decodeJson(trimmed);

  if (Option.isSome(direct)) {
    return direct.value;
  }

  const lines = trimmed.split('\n');

  return parseFromLines(lines);
}
