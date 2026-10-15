# Changelog

## v0.9.0

- Diff checks ask whether the change introduces, extends, or worsens a violation compared with the before contents,
  not whether the resulting code violates the contract. A violation the base already had no longer fails an unrelated
  edit to the same file. Another instance, a copy, or a violation created by moving code still fails. Full checks are
  unchanged.
- Large changes still reach the model. When a file input exceeds 64 KiB, Tenet leaves out its prior and then its
  resulting contents while the patch still shows the change. The combined input also counts omitted inventory paths
  per directory and leaves out the contents of the largest changed files. `omitted` and `context` list each reduction.
- File checks run even when the combined input cannot fit. A file check that finds a violation fails the contract with
  `file_violation` when the combined assessment is uncertain or cannot run; otherwise the contract stays unresolved.
- Skip empty, binary, non-UTF-8, and symbolic-link files instead of exiting 2. They stay in the inventory, and
  `export` lists them as `unreadable`. A file whose patch alone exceeds the limit is `incomplete`. `check --dry-run`
  reports empty files as skipped too.
- Report `violation`, the model's probability of a violation, in place of routing confidence in conclusions, the
  review log, and `status`. `status` suggests the pending contract the model leans hardest toward violating first.
- `check --dry-run` reports each contract's combined input size and reductions, and exits 2 only for execution errors.
  Its summary predicts the requests a check would send and counts contracts whose combined input exceeds the limit
  after reduction, and it warns on stderr when the prediction exceeds `--max-requests`. A 94-file change predicts
  1,022 requests, 17 of 28 combined inputs over the limit; commit 521c054 predicts 1,051. A 2-file change predicted 41
  and sent 41. A diff with no changes prints `No changes since BASE; nothing to check.` A check that reaches the budget
  says how many requests it did not send, and the summary's `over_budget` counts them.
- Context summaries count omitted inventory files instead of listing them, and file results drop `source_hash`.
  Verbose file states read `NOT APPLICABLE` and `UNCERTAIN`, matching JSON.
- Discover files in folders named `target`, `node_modules`, `dist`, `out`, `.context`, and `.agents` at any depth.
  Only `.git`, `.tenet`, `.tenet.lock`, `.env`, and `.env.*` are excluded by name; ignore files exclude everything
  else. Nested repositories and worktrees, such as `.claude/worktrees/`, are skipped instead of failing on duplicate
  contract names. A source folder such as `src/out/` is no longer skipped. Add build output to `.gitignore` or `.ignore` if
  it is not already listed there.
- Add an optional `paths` frontmatter list of glob patterns, relative to the contract's scope directory. A contract with
  `paths` assesses, inventories, and exports only matching files, and `contracts list --for` reports it only for them. A
  pattern that starts with `!` excludes the files it matches, such as a committed lockfile that ignore files cannot hide
  from a diff; a list of only exclusions governs every other file. Files inside `.contracts` are subjects only for a
  pattern that names `.contracts`. A diff that changes none of a contract's files, and not the contract, concludes
  `unaffected` without a model request. `tenet validate` rejects an empty list and absolute, parent-relative, or invalid
  patterns, including exclusions.
- A positional selection no longer runs a contract that governs none of the selected files. `check` and `export`
  warn on stderr when a selected path yields nothing to assess, instead of exiting 0 silently.
- Evidence packets move to `version: 2`. `inventory` lists only files the exported contracts govern, and the
  top-level `omitted_files` list is removed; each contract keeps its own `omitted_files`.
- Keep a review on the commit `check --base` resolved. `status` no longer resolves the ref again, so committing the
  change keeps its pending and recorded contracts. The review's label names the base commit and selected paths, and
  `status` says when the working tree has no changes. `status --base REF` reports the review against that commit and
  exits 4 when no check compares the working tree with it. An unknown `--base` names the ref instead of Git's
  `Needed a single revision`.
- A diff check's input no longer includes the list of files in each contract's scope, because a diff assumes the base
  complies. Rebasing onto a base that only gains an unrelated file keeps conclusions and records: in a 17-contract
  change rebased onto a base that added `workspaces/core/NOTES.md`, `check --no-model` reused 0 of 17 conclusions and
  neither record before, and 17 of 17 and both records after. Full checks still include the list, so a new file in
  scope leaves a contract pending. Diff records made before this change show as outdated once; check and record again.
- Checks against the same base commit form one review. A later check replaces an earlier assessment of a contract only
  when it selects every earlier path or the contract's whole scope; otherwise both stand, and the contract is settled
  only when each is. `status` notes an earlier check's paths, and `show` and `record` take them after the name. In a
  diff review, a check that names one contract leaves the change's other contracts `not_checked`.
- Show records that override the model. `check` and `status` print `recorded ID over model failed · violation N%` and
  count contracts settled by a record; `status --json` adds `model_violation`, `model_answer`, `selection`, and
  `min_confidence`. An unresolved row shows the model's lean, as in `leans no violation · confidence 0.56 < 0.8`, and
  logged conclusions keep the answer's choice and confidence.
  `check --no-model` rows and JSONL `contract_finished` events carry the reused conclusion, with `cached: true`.
  The summary's duration covers the whole check, from discovery to the review update, so `--no-model` no longer
  reports 0.00 s.
- `record` accepts evidence only in files the check assessed, under the `--context` path rules, with or without a
  leading `./`. In a diff, a favorable record must cite a line the change wrote, a line beside one, or a file it deleted
  or emptied, and one over the model's `failed` must also cite a file the model flagged. When the diff changed none of
  the contract's files, a favorable record instead needs a `verified` full check of the same contract text. Messages are
  limited to 4 KiB of text without control, bidirectional, or line separator characters other than newlines and tabs.
  `record` names the record it replaces when the conclusions differ. Reading the log rejects a record line that
  `record` would refuse, names the line, and says to move `.tenet` aside, run `check`, and record the review again;
  records that 0.8.0 wrote are checked only for the revision and input they name. JSON errors carry the full cause.
- `check`, `status`, `show`, `record`, and `log` exit 2 while Git tracks a file in `.tenet/`, and say how to untrack
  it.
- A diff that adds or edits a contract leaves it pending as `contract_changed` despite a favorable answer. `check`,
  `status`, and the JSONL `begin` event list contract documents the diff deletes, including one moved to a path that
  is not a contract.
- Each revision records `--min-confidence`; `status`, `show`, and `log` print a threshold other than 0.8.
- Each revision stores its captured files once, and its assessments refer to them. A 58-contract check of this
  repository logs 198 KB instead of 1.85 MB. Diff revisions also record each file's changed lines. Tenet reads 0.8.0
  logs and appends the new format to them; Tenet 0.8.0 cannot read a log 0.9.0 has written. `.tenet/blobs` no longer
  keeps files over 1 MiB. Commands read the log once: at 12,500 revisions, `status` takes 0.8 s instead of 12.6 s and
  `log` 0.8 s instead of 71 s.
- For a scope over 20 files, `show` counts files and file results by kind and lists only those that need attention,
  naming five unreadable files and counting the rest. It lists the latest five records and assessments; `log NAME`
  lists the rest. `show --pending` prints every pending contract and their shared change once. `show --json` and `log --json` move to
  `version: 2`, and `show --json` lists the scoped files once instead of in every assessment.
- `status` suggests the check command, with the review's base commit, contract, and context, for contracts captured
  without a model. In a diff review, `show` prints each file's change once. `tenet log | head` prints no broken-pipe
  error.
- The Reviews page's Claude Code recipe pins the session's starting commit, and its stop hook runs `status --base`
  without a model. It blocks once per stop, using Claude Code's `stop_hook_active`, while a contract is pending or the
  review cannot be read. The page states that the hook is a reminder, not enforcement.
- `contracts add --source tenet=PATH` says that `tenet` always names the public library and suggests another alias,
  instead of claiming `.tenet.lock` records it when no lock exists.

## v0.8.0

- Keep review state in `.tenet/`, which ignores itself. Each `check` appends a revision to `events.jsonl` with every
  contract's exact input and conclusion. Unchanged requests reuse answers from `answers.jsonl`; `--fresh` asks again.
- Add `tenet status`, `show`, `record`, and `log`. `status` reports each contract settled or pending without writing or
  calling a model. `record` settles a contract's current input with a conclusion, cited lines, and a message, and
  refuses input that changed after the latest check. An outdated record shows the changes since it was made.
- A model's `failed` or `unresolved` conclusion stays pending until a record settles it. `check` and `status` exit 4
  while any contract is pending, 1 for a recorded failure, and 3 for a recorded `unresolved` conclusion.
- Add `check --no-model` to capture a revision without credentials or requests.
- Rename `tenet evidence` to `tenet export` and `tenet contracts view` to `tenet contracts show`. The packet format is
  unchanged. JSONL output moves to version 4 with each contract's `state` and the summary's `revision` and `pending`.
- Trust the operating system's certificate store as well as bundled roots, through ev-grep 0.6.3. Checks behind a
  TLS-inspecting proxy no longer fail with `Jev transport failed`; `SSL_CERT_FILE` and `SSL_CERT_DIR` apply when set.

## v0.7.0

- Install shared contracts with `tenet contracts add tenet/NAME --scope DIR`. Contracts come from the public
  [tenet-contracts](https://github.com/macromackie/tenet-contracts) library or any Git source registered with
  `--source ALIAS=URL`. A committed `.tenet.lock` records each copy's release, commit, and content hash.
- Take library releases with `tenet contracts outdated` and `tenet contracts update`. Unedited copies are replaced,
  edited copies merge three ways, and overlapping edits leave diff3 conflict markers with exit 1.
  `tenet contracts diff` and `tenet contracts remove` inspect and drop copies.
- `tenet validate` rejects unresolved conflict markers in contracts and checks `.tenet.lock`. Discovery skips the lock,
  and `check` and `evidence` never read it.

## v0.6.6

- Classify oversized source files, including the base revision, as unresolved `context_too_large` instead of an
  unavailable-source error. Preserve the path and byte limit without truncating source or spending model requests.
  Unreadable and binary source still report execution errors.

## v0.6.5

- Limit diff contract discovery to scopes containing changed or renamed files. A malformed contract in an unrelated folder no longer blocks the diff's inventory or check.
- Keep validation strict for affected scopes, explicit named contracts, and repository-wide validation.

## v0.6.4

- Size-check complete contract context before file assessments. Oversized scopes remain unresolved without spending model requests, preserving budget for other contracts.
- Retain actual and allowed bytes, source scope, and evidence identity on oversized-context results. Captured source failures still block execution and name the affected file.
- Reuse the prepared evidence for the final assessment. Model questions, fitting inputs, confidence routing, and request limits are unchanged.

## v0.6.3

- Reduce repeated inventory paths in combined assessments by using directory trees when they encode to fewer bytes.
  Smaller inventories retain expanded path lists, and tree nesting is bounded.
- Preserve exact inventory, selected, supporting, and omitted paths alongside complete captured source and changes.
  JSONL scope diagnostics and the version 1 offline evidence format are unchanged; combined input sizes and hashes
  reflect the chosen encoding. Model questions, confidence routing, and request limits are unchanged.

## v0.6.2

- Speed up scope checks for large file selections and avoid repeated scope calculations during JSON dry runs.
  Selected files, scope classifications, and report contents are unchanged.

## v0.6.1

- Retain every selected captured source in combined assessments, including files classified as not applicable.
  Oversized combined inputs remain unresolved instead of dropping source to fit the request limit.
- Preserve a routed file violation when its relevance answer disagrees. A contrary combined no-violation answer
  reports unresolved with the conflicting files; both raw answers remain visible.
- Existing prompts, confidence policy, requested scopes, and input limits are unchanged.

## v0.6.0

- Export review inputs offline with `tenet evidence`: complete contracts, captured before/after source, content hashes,
  selected and supporting paths, and omissions. The JSON packet defaults to a 1 MiB limit; no model call is made.
- Capture source once per run and reuse the same contents or read errors across file and combined assessments.
  Capture has a fixed 64 MiB aggregate payload bound and fails before assessment when exceeded. Sequential capture
  is not an atomic filesystem snapshot.
- Honor `.ignore` for untracked diff inputs while retaining tracked changes, deletions, renames, and source inside
  newly added nested repositories.
- Use ev-grep 0.6.1. Existing model prompts and the default confidence floor are unchanged.
- Rust API: call `Plan::capture().await?` and obtain file inputs from its `EvidencePacket`. Use
  `Plan::export_evidence(max_bytes).await?` for the offline JSON representation.

## v0.5.0

- Assess selected files together with explicit helper context. Per-contract scope recognizes complete nested selections;
  favorable partial selections report `clear` without claiming full verification.
- Apply configurable `--min-confidence` (default 0.8) consistently. Preserve raw model answers and separate routing
  reasons instead of promoting weak answers to file passes or failures.
- Report unresolved claims through exit 3. Ordinary output includes scope, reasons, and candidate paths. JSONL v3
  records selected, supporting, and omitted source alongside the combined assessment.
- Keep combined diff evidence complete with before contents and patches; deduplicate explicit supporting source.
- Use ev-grep 0.5.0's raw assessment API and content-derived prompt identities.

## v0.4.7

- Validate complete file inputs during credential-free dry runs and before counting model requests. Oversized inputs report byte limits without truncating source.
- Add structured conclusion reasons and repository context sizes and omitted paths to JSONL reports, including provider failures.
- Clarify how reviewers combine targeted behavioral search, related implementations, and deterministic checks. Model questions and judgment policy are unchanged.

## v0.4.6

- Include conclusion reasons, raw model assessments, and evidence hashes in JSONL. Consumers can distinguish
  limited coverage, missing context, model uncertainty, and conflicting judgments without losing the original answer.
- Explain how to investigate unresolved results and preserve reports when supplying related context.

## v0.4.5

- Judge each file against the contract's stated scope, which may cover code, configuration, tests, or documentation.
  Notes that describe a product no longer fall outside a documentation contract.
- Send each resulting file to the model once. File-level inputs are 13–38% smaller on the evaluation fixtures.
- Use ev-grep 0.4.4, which retries a model response that fails validation, such as probabilities that do not sum to
  one, instead of ending the check with an execution error.
- Stop parsing `## Applies to` separately. The model reads it with the rest of the contract, and an empty section
  no longer fails validation.
- Document optional agent guidance, targeted questions with ast-grep and ev-grep, run-to-run variation, one concern
  per contract, and a JSONL gate that also fails on unresolved contracts.

## v0.4.4

- Ask relevance and violation questions together, with shared before/after context.
- Check independent contracts concurrently within one worker limit. Print final summaries in contract order.
- Keep unrelated source out of repository assessments and surface conflicting file assessments as unresolved.
- Include request attempts, hedges, timing, and available usage in JSON reports.

## v0.4.3

- Use ev-grep 0.4.0 for the shared provider adapter.

## v0.4.2

- Reserve context space for required source and JSON metadata before adding optional neighboring files. Small changes in larger repositories can now reach the final assessment without optional context exhausting the request limit.

## v0.4.1

- Avoid duplicating complete added files as patches in model inputs.


## v0.4.0

- Read whole contract documents, including exceptions and project check instructions.
- Report contract assessments with model confidence in JSONL v2.
- List scoped contracts as JSON and add related files with `check --context`.
- Combine source across files before deciding a contract failed.
- Evaluate repository fixtures with optional related-file context.


## v0.3.1

- Use `--endpoint` or `TENET_ENDPOINT` to run checks through a trusted provider proxy.

## v0.3.0

- `check --base REF` assesses before/after changes under a valid-base assumption. It replaces `--changed-since`. Full checks also assess repository context, including missing required files.
- `check --snapshot DIR --patch FILE` checks a copied repository without modifying the original.
- `eval FIXTURES` runs repositories and patches against expected contract conclusions. Use `--case` to select a case and `--validate` to check fixtures offline. Reports are saved under `.tenet/evals/`.
- Terminal output shows one result per contract with steady progress. Only failures include requirement snippets. Use `--reporter verbose` for file judgments.
- Unresolved contracts exit 0. Failures exit 1; execution errors and incomplete checks remain nonzero. Evaluation mismatches exit 0 unless `--strict` is set.

See [Evaluation](https://tenet-contracts.com/docs/evaluation) and [Output](https://tenet-contracts.com/docs/output) for the updated formats.
