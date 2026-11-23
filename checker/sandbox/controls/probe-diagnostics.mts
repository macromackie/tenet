import { Option } from 'effect';
import type { Check } from '../../types.d.mts';
import type { ProjectDeps } from '../project-deps.mts';
import { detectMissingExternalPackage } from './missing-package.mts';
import { stripControlChars } from './strip-control-chars.mts';

const DIAGNOSTIC_LINES = 2;
const DIAGNOSTIC_CHARS = 240;

const DIAGNOSTIC_FOOTER =
  /^(?:error: aborting due to |For more information about this error|Node\.js v\d)/v;

const DIAGNOSTIC_ERROR =
  /^(?:error(?:\[|:)|Error\b|TypeError:|RangeError:|ReferenceError:|SyntaxError:|URIError:|EvalError:|ModuleNotFoundError:|ImportError:|AttributeError:|ValueError:|KeyError:|AssertionError:|NameError:|RuntimeError:|ZeroDivisionError:|FileNotFoundError:|OSError:|Exception:|panic:|cannot find package|no required module provides package|missing go\.sum entry|\.?\/?[^\s:]+\.go:\d+:)/v;

const RUST_MISSING_CRATE_PATTERN = /error\[E04(?:32|63)\]/v;
const GO_MISSING_MODULE_PATTERN = /no required module provides package|cannot find package/v;
const TAIL_DIAGNOSTIC_RUNTIMES: ReadonlySet<string> = new Set(['node', 'python']);

type FailureInput = Readonly<{
  base: string;
  deps?: ProjectDeps;
  filePaths?: readonly string[];
  runtime?: Check['runtime'];
  stderr: string;
}>;

type MissingDepHintInput = Readonly<{
  deps?: ProjectDeps;
  filePaths?: readonly string[];
  runtime?: Check['runtime'];
  stderr: string;
}>;

type SelectLinesInput = Readonly<{
  lines: readonly string[];
  runtime?: Check['runtime'];
}>;

type SummarizeInput = Readonly<{
  runtime?: Check['runtime'];
  stderr: string;
}>;

function selectDiagnosticLines({ lines, runtime }: SelectLinesInput): readonly string[] {
  const highlighted = lines.filter((line) => DIAGNOSTIC_ERROR.test(line));

  if (highlighted.length > 0) {
    if (runtime !== undefined && TAIL_DIAGNOSTIC_RUNTIMES.has(runtime)) {
      return highlighted.slice(-DIAGNOSTIC_LINES);
    }

    return highlighted.slice(0, DIAGNOSTIC_LINES);
  }

  return lines.slice(-DIAGNOSTIC_LINES);
}

function summarizeDiagnostic({ runtime, stderr }: SummarizeInput): string {
  const lines = stderr
    .split('\n')
    .map((line) => line.trim())
    .filter((line) => line.length > 0 && !DIAGNOSTIC_FOOTER.test(line));

  const selected = selectDiagnosticLines({ lines, runtime }).join(' | ');
  const summary = stripControlChars(selected).trim();

  if (summary.length <= DIAGNOSTIC_CHARS) {
    return summary;
  }

  return summary.slice(0, DIAGNOSTIC_CHARS);
}

function readMissingDepHint({ deps, filePaths, runtime, stderr }: MissingDepHintInput): string {
  const externalPkg = detectMissingExternalPackage({ deps, filePaths, runtime, stderr });

  if (Option.isSome(externalPkg)) {
    const pkgLabel = JSON.stringify(externalPkg.value);

    return ` (missing external package ${pkgLabel} is not installed in the sandbox environment)`;
  }

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
export function formatProbeFailure({
  base,
  deps,
  filePaths,
  runtime,
  stderr,
}: FailureInput): string {
  const detail = summarizeDiagnostic({ runtime, stderr });
  const hint = readMissingDepHint({ deps, filePaths, runtime, stderr });

  if (detail.length === 0) {
    return `${base}${hint}`;
  }

  return `${base}: ${detail}${hint}`;
}
