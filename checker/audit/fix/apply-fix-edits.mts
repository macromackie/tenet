import { Option, Result, Schema } from 'effect';
import { applyControl } from '../../sandbox/index.mts';

const MAX_PATCHED_FILE_BYTES = 524_288;
const MAX_PROPOSAL_EDITS = 24;
const OptionalLine = Schema.OptionFromOptionalKey(Schema.Int);

const FixEditFields = Schema.Struct({
  find: Schema.String,
  line: OptionalLine,
  path: Schema.String,
  replace: Schema.String,
});

const FixEdit = Schema.toEncoded(FixEditFields);
type FixEdit = typeof FixEdit.Type;

const FixEditList = Schema.Array(FixEdit);

const FixProposalFields = Schema.Struct({
  edits: FixEditList,
  summary: Schema.String,
});

const FixProposal = Schema.toEncoded(FixProposalFields);
type FixProposal = typeof FixProposal.Type;

const decodeFixProposal = Schema.decodeUnknownOption(FixProposal, { onExcessProperty: 'error' });

type ApplyAuditFixInput = Readonly<{
  allowedFiles: readonly string[];
  files: ReadonlyMap<string, string>;
  raw: unknown;
}>;

type ApplySingleFixEditInput = Readonly<{
  edit: FixEdit;
  source: string;
}>;

type ValidateEditsInput = Readonly<{
  allowedPaths: ReadonlySet<string>;
  edits: readonly FixEdit[];
  files: ReadonlyMap<string, string>;
}>;

type ApplyValidatedEditsInput = Readonly<{
  edits: readonly FixEdit[];
  files: ReadonlyMap<string, string>;
  summary: string;
}>;

type ValidateSingleTargetInput = Readonly<{
  allowedPaths: ReadonlySet<string>;
  files: ReadonlyMap<string, string>;
  path: string;
}>;

type ApplyAuditFixResult =
  | Readonly<{
      changedPaths: readonly string[];
      files: Map<string, string>;
      ok: true;
      summary: string;
    }>
  | Readonly<{
      error: string;
      ok: false;
    }>;

function buildControlItem(edit: FixEdit): Readonly<Record<string, unknown>> {
  if (edit.line !== undefined) {
    return { find: edit.find, line: edit.line, replace: edit.replace };
  }

  return { find: edit.find, replace: edit.replace };
}

function readPatchError(cause: unknown): string {
  if (cause instanceof Error && cause.message.length > 0) {
    return cause.message;
  }

  return 'could not apply proposed edit';
}

function applySingleFixEdit({ edit, source }: ApplySingleFixEditInput): Result.Result<string, string> {
  const controlItem = buildControlItem(edit);

  function runPatch(): string {
    return applyControl({
      control: [controlItem],
      label: edit.path,
      limit: MAX_PATCHED_FILE_BYTES,
      source,
    });
  }

  return Result.try({ catch: readPatchError, try: runPatch });
}

function validateEditTarget({
  allowedPaths,
  files,
  path,
}: ValidateSingleTargetInput): Option.Option<string> {
  if (!allowedPaths.has(path)) {
    const target = JSON.stringify(path);

    return Option.some(`edit path ${target} is outside allowed group files`);
  }

  if (!files.has(path)) {
    const target = JSON.stringify(path);

    return Option.some(`edit target ${target} was not loaded in the execution set`);
  }

  return Option.none();
}

function validateProposedEdits({
  allowedPaths,
  edits,
  files,
}: ValidateEditsInput): Option.Option<string> {
  if (edits.length === 0) {
    return Option.some('fix proposal returned no edits');
  }

  if (edits.length > MAX_PROPOSAL_EDITS) {
    return Option.some('fix proposal exceeded the maximum edit count');
  }

  for (const edit of edits) {
    const issue = validateEditTarget({ allowedPaths, files, path: edit.path });

    if (Option.isSome(issue)) {
      return issue;
    }
  }

  return Option.none();
}

function applyValidatedEdits({
  edits,
  files,
  summary,
}: ApplyValidatedEditsInput): ApplyAuditFixResult {
  const patchedFiles = new Map<string, string>(files);
  const changedPaths = new Set<string>();

  for (const edit of edits) {
    const current = patchedFiles.get(edit.path) ?? '';
    const updated = applySingleFixEdit({ edit, source: current });

    if (Result.isFailure(updated)) {
      return { error: `${edit.path}: ${updated.failure}`, ok: false };
    }

    patchedFiles.set(edit.path, updated.success);
    changedPaths.add(edit.path);
  }

  return { changedPaths: [...changedPaths], files: patchedFiles, ok: true, summary };
}

/** Decode a proposed fix JSON payload, reject edits outside allowed files, and patch in memory. */
export function applyAuditFix({
  allowedFiles,
  files,
  raw,
}: ApplyAuditFixInput): ApplyAuditFixResult {
  const decoded = decodeFixProposal(raw);

  if (Option.isNone(decoded)) {
    return { error: 'fix proposal did not match expected { summary, edits } JSON', ok: false };
  }

  const allowedPaths = new Set<string>(allowedFiles);
  const invalid = validateProposedEdits({ allowedPaths, edits: decoded.value.edits, files });

  if (Option.isSome(invalid)) {
    return { error: invalid.value, ok: false };
  }

  return applyValidatedEdits({
    edits: decoded.value.edits,
    files,
    summary: decoded.value.summary,
  });
}
