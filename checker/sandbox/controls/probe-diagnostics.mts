import type { Check } from '../../types.d.mts';
import type { ProjectDeps } from '../project-deps.mts';

const DIAGNOSTIC_LINES = 2;
const DIAGNOSTIC_CHARS = 240;
const ASCII_SPACE = 32;
const ASCII_DEL = 127;

const DIAGNOSTIC_FOOTER =
  /^(?:error: aborting due to |For more information about this error|Node\.js v\d)/v;

const DIAGNOSTIC_ERROR =
  /^(?:error(?:\[|:)|Error\b|TypeError:|RangeError:|ReferenceError:|SyntaxError:|URIError:|EvalError:|ModuleNotFoundError:|ImportError:|AttributeError:|ValueError:|KeyError:|AssertionError:|NameError:|RuntimeError:|ZeroDivisionError:|FileNotFoundError:|OSError:|Exception:|panic:|cannot find package|no required module provides package|\.?\/?[^\s:]+\.go:\d+:)/v;

const RUST_MISSING_CRATE_PATTERN = /error\[E04(?:32|63)\]/v;
const GO_MISSING_MODULE_PATTERN = /no required module provides package|cannot find package/v;

type FailureInput = Readonly<{
  base: string;
  deps?: ProjectDeps;
  runtime?: Check['runtime'];
  stderr: string;
}>;

type MissingDepHintInput = Readonly<{
  deps?: ProjectDeps;
  runtime?: Check['runtime'];
  stderr: string;
}>;

function stripControlChars(text: string): string {
  let clean = '';

  for (const character of text) {
    const code = character.codePointAt(0) ?? 0;

    if (code >= ASCII_SPACE && code !== ASCII_DEL) {
      clean += character;
    } else {
      clean += ' ';
    }
  }

  return clean;
}

function selectDiagnosticLines(lines: readonly string[]): readonly string[] {
  const highlighted = lines.filter((line) => DIAGNOSTIC_ERROR.test(line));

  if (highlighted.length > 0) {
    return highlighted.slice(0, DIAGNOSTIC_LINES);
  }

  return lines.slice(-DIAGNOSTIC_LINES);
}

function summarizeDiagnostic(stderr: string): string {
  const lines = stderr
    .split('\n')
    .map((line) => line.trim())
    .filter((line) => line.length > 0 && !DIAGNOSTIC_FOOTER.test(line));

  const selected = selectDiagnosticLines(lines).join(' | ');
  const summary = stripControlChars(selected).trim();

  if (summary.length <= DIAGNOSTIC_CHARS) {
    return summary;
  }

  return summary.slice(0, DIAGNOSTIC_CHARS);
}

function readMissingDepHint({ deps, runtime, stderr }: MissingDepHintInput): string {
  if (
    runtime === 'rust' &&
    deps?.rustDepsDir === undefined &&
    RUST_MISSING_CRATE_PATTERN.test(stderr)
  ) {
    return ' (run cargo build locally so compiled crate deps exist in target/debug/deps)';
  }

  if (runtime === 'go' && deps?.goVendor === undefined && GO_MISSING_MODULE_PATTERN.test(stderr)) {
    return ' (run go mod download or go mod vendor locally so Go module dependencies are cached)';
  }

  return '';
}

/** Format a concise, sanitized probe failure message with actionable dependency hints. */
export function formatProbeFailure({ base, deps, runtime, stderr }: FailureInput): string {
  const detail = summarizeDiagnostic(stderr);
  const hint = readMissingDepHint({ deps, runtime, stderr });

  if (detail.length === 0) {
    return `${base}${hint}`;
  }

  return `${base}: ${detail}${hint}`;
}
