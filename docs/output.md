# Output

The default reporter shows progress while files are assessed, then one final result per contract. Unresolved results show the reason they could not complete. Only failed contracts show requirement snippets and finding details. Interactive terminals show a spinner, file counts, and elapsed time below completed contracts. Failure details follow the results. Redirected output contains no animation.

Use `--reporter verbose` for individual file judgments. JSONL retains all intermediate evidence, including uncertainty that a later repository assessment resolves.
Human reports go to stderr. JSONL goes to stdout.
The offline [`export` command](./export.md) has a separate JSON format containing source and contract documents.

```sh
tenet check --base origin/main --reporter jsonl > report.jsonl
```

## Contract conclusions

| Conclusion | Meaning |
| --- | --- |
| verified | Full-check evidence supports compliance |
| preserved | Relevant changes preserve compliance, assuming a valid base |
| unaffected | No relevant changes found across all selected contract changes, assuming a valid base |
| clear | No violation found in the selected subjects; the full contract scope was not verified |
| failed | Evidence supports a violation |
| unresolved | The model cannot decide, or required evidence or execution is incomplete |

A full check cannot conclude unaffected or preserved. A diff check cannot newly verify the baseline.
These are model judgments, not formal proofs. No findings alone is insufficient for verification.

## Review states

Each contract is settled or pending for the exact input a check captured: the contract text, mode, scope, and the
content of every scoped file. A full check's input also lists every file in the contract's scope, so a new file leaves
it pending. A diff check's does not, because it assumes the base complies: rebasing onto a base that only gains
unrelated files keeps its conclusions. A record from `tenet record` settles its input. Otherwise a model's `verified`,
`preserved`, `unaffected`, or `clear` settles it. Every other contract is pending:

| Reason | Meaning |
| --- | --- |
| `model_failed` | The model found a violation; confirm or dispute it with a record |
| `model_unresolved` | The model could not decide, or its evidence or execution was incomplete, such as a [failed request](#failed-requests); `status` and `show` print its lean, as in `leans no violation · confidence 0.56 < 0.8` |
| `not_assessed` | `check --no-model` captured the input without asking the model |
| `inputs_changed` | A scoped file or the contract changed after the latest check (`status` only) |
| `not_checked` | No check assessed the contract: it joined the review later, or a diff check named another contract (`status` only) |
| `contract_changed` | The diff adds or edits the contract, so a favorable diff answer cannot settle it |

A record or conclusion never carries to a different input. `tenet show` lists outdated ones with the changes since.

File results are `pass`, `fail`, `not_applicable`, `uncertain`, `error`, `incomplete`, or `skipped`.
`skipped` marks a file with no text to judge: empty, binary, not UTF-8, or a symbolic link. `incomplete` marks a file the request budget did not reach, or whose input exceeds the limit even with only its patch.
After confidence routing, a violation `match` produces `fail` even when relevance says `no_match`. Otherwise, relevance `no_match`
produces `not_applicable`; that status retains the source for the combined assessment. For other relevance outcomes,
violation `no_match` produces `pass` and uncertain violation produces `uncertain`. Raw answers and routing remain visible.

The verbose reporter shows uncertain file judgments as `UNCERTAIN`. A repository assessment can resolve them when combined context supplies the missing evidence; the original file judgments remain in the report. Operational errors and exhausted budgets retain separate counts and exit codes.

## JSONL

Each event has `version: 4` and a `type`. The `begin` event records `full` or `diff` mode, provider/model (`null` for `--no-model`), root, base and HEAD when available, the confidence threshold, the baseline assumption, and `deleted_contracts`, the contract documents a diff deletes. Snapshot runs include the original snapshot path and patch hash.

`result` contains file evidence: mode, change kind, previous path for renames, before/after hashes, contract hash, judgments, and timing. `omitted` lists `before` or `contents` when they were left out to fit the limit; the patch remains. `input_bytes` measures the encoded state including contract and explicit context, before provider instructions and question wrappers; it is not a token count.

`contract_finished` includes the contract's `state`, such as `{"state": "pending", "reason": "model_failed"}` or
`{"state": "settled", "conclusion": "failed", "record": "d370e7dc32d8"}`, and the model's `conclusion`:

```json
{
  "assessment": "clear",
  "scope": "selected_subjects",
  "violation": 0.03,
  "reason": "No violation found in the selected subjects; this does not verify the full contract scope.",
  "reason_code": "selected_subjects_clear",
  "request": null
}
```

Under `check --no-model`, `conclusion` is the earlier conclusion for the same input with `cached: true`, or `null`
when there is none.

`violation` is the probability the model gave a violation in the requested scope. When no combined assessment ran,
it is the highest probability among the file checks. `status` and `show` print it as `violation N%`; `status` suggests the pending
contract with the highest value first. It is a model estimate, not a measured accuracy.
The raw `choice`, `confidence`, and probabilities stay unchanged in `model_assessment`. `routing` separately records the outcome after
applying `--min-confidence` (default 0.8). Low-confidence and explicit uncertain answers route to unresolved.
File results retain the same distinction in `applicability_routing` and `verification_routing`. When there is no final
model answer, `violation` is null. Errors add an `error` field.

`reason` explains the conclusion, including selected-path coverage, unavailable context, model uncertainty, and
conflicting judgments. When a repository assessment ran, `model_assessment` preserves its answer and probabilities,
and `evidence_hash` identifies its encoded input without printing source. `conflicting_files` lists file failures
that contradict a favorable repository answer. Those disagreements remain unresolved. When the repository answer is
uncertain or could not run, a file `fail` fails the contract with `file_violation`.

`reason_code` supports routing without parsing prose:

| Code | Next step |
| --- | --- |
| `file_checks_incomplete` | Inspect file errors or incomplete results |
| `selected_subjects_clear` | Review the remaining scope before claiming full verification |
| `no_relevant_changes` | Retain the compliant-base assumption |
| `request_budget` | Account for unassessed work before increasing the budget |
| `context_too_large` | Read the file checks, then narrow paths or review the remaining scope manually |
| `context_unavailable` | Inspect the source-read error |
| `assessment_failed` | Inspect the provider error |
| `model_uncertain` | Investigate missing evidence and the raw assessment |
| `conflicting_assessments` | Trace the flagged files against the combined evidence |
| `file_violation` | Confirm the flagged file's violation |
| `violation` / `compliance` | Review the model's conclusion and evidence |

When combined context was assembled, `context` reports `input_bytes`, `limit_bytes`, `included_files`,
`selected_files`, `support_files`, and `omitted_files`, the number of scoped inventory files whose contents were not
supplied. `excerpts` maps each support file supplied as an [excerpt](./reviews.md#add-context) to its line ranges,
and each selected file whose excerpt took the place of contents left out to fit.
All captured selected source in the contract scope is retained regardless of relevance answers. When the
context was [reduced to fit](./limits.md), `before_omitted`, `inventory_counted`, and `contents_omitted` say how.
These fields describe the supplied state, not proof that it contains every necessary implementation.
The same diagnostics and evidence hash remain available if the provider fails. `input_bytes` and `evidence_hash`
describe the actual encoded assessment input.

Version 4 adds each contract's `state`, the summary's `revision` and `pending` contracts, and `cached` counts for
results answered from earlier identical requests. Version 3 replaced the run-wide `partial` flag with each conclusion's
`scope` and separated raw answers from routing. Consumers should retain useful diagnostics and ignore unknown fields; see [JSON](./json.md#compatibility). A reason describes the evidence limit; it does not turn an unresolved assessment into a pass.

Other events include `contract_started`, `check_started`, `stage_started`, `stage_completed`, `summary`, and `error`. Stages are `applicability`, `verification`, and `completeness`.
The summary includes file counts, contract counts, request and cached counts, `over_budget` for requests that
`--max-requests` stopped, `request_failures` and `retryable_failures`, `elapsed_ms` for the whole check, the appended
`revision` (`null` for a snapshot), the `pending` contract names, `coverage`, and the exit code. `errors` counts every
execution error, failed requests included. Source contents and credentials are not printed.

## Failed requests

Jev retries a failed request within its own [budget](./limits.md). When a request still returns no answer, its file
result is `error`, or the conclusion's `reason_code` is `assessment_failed` when the combined assessment failed. The
contract stays pending unless a record already settles its input. The `result` event, or the conclusion for a combined
assessment, adds a `failure` object:

```json
{ "kind": "rate_limited", "retryable": true }
```

`retryable` says whether rerunning the same check may answer the request:

| Kind | Cause | Retryable |
| --- | --- | --- |
| `timeout` | The request used its 15-second budget, or the provider answered HTTP 408 | yes |
| `connection` | No attempt received an HTTP response | yes |
| `rate_limited` | HTTP 429 | yes |
| `interrupted` | HTTP 499 | yes |
| `server_error` | HTTP 5xx | yes |
| `authentication` | HTTP 401 or 402: the provider rejected the credential or its account | no |
| `configuration` | HTTP 404, 405, or a redirect: the endpoint or model does not exist | no |
| `input_too_large` | HTTP 413, or the encoded request exceeds 96 KiB | no |
| `invalid_request` | Any other HTTP status | no |
| `unknown` | A failure Tenet does not recognize, such as an answer that failed validation three times | no |

An `authentication` or `configuration` failure stops the check at once, because every other request would fail too.
It exits 2 like other fatal errors and appends no revision. JSONL also writes one line to stderr for each failed request, naming the
contract, the file or combined assessment, the kind, and whether it is retryable. A fatal error appears both as the
`error` event and on stderr.

Answers are cached when they arrive, and a failed request caches nothing. Rerunning the check therefore sends only the
failed requests, plus any whose input changed; `check --dry-run` predicts them. A `--snapshot` check keeps no answers,
so a rerun sends every request again.

## Coverage

In a diff check, the summary's `coverage` reports how much of the selected change any contract governs, whichever
contracts the check runs:

```json
{ "changed_files": 271, "ungoverned_files": ["README.md", "docs/setup.md"], "governed": true }
```

`ungoverned_files` lists every changed file that no contract governs at its current or previous path. Files in a
`.contracts` folder are contract changes, not ungoverned files. `governed` is false when no contract applies to the change,
including a diff with no changes. A full check reports `null`, because it checks contract scopes rather than a change.
The default reporter prints `No contract governs this change: 3 changed files`, or names the ungoverned files when
contracts govern the rest. A check that no contract governs still exits 0 and needs no credential: nothing in the review is pending.

## Check exit codes

`check` and `status` share these codes:

| Code | Meaning |
| --- | --- |
| 0 | Every contract is settled without a violation |
| 1 | A settled violation: a recorded failure |
| 2 | Invalid arguments or configuration, credentials or an endpoint the provider rejected, unreadable review state, a capture failure, or another execution error |
| 3 | A recorded `unresolved` conclusion |
| 4 | A contract is pending, including model failures, unresolved answers, failed requests, and exhausted budgets |
| 130 | Cancelled |

Cancellation takes precedence, then execution errors, pending contracts, violations, and recorded unresolved
conclusions. A model's `failed` exits 4 until a record confirms it. A [failed request](#failed-requests) leaves its
contract pending, so the check exits 4, unless a record settles the contract or the failure stopped the check. Exit 0 can include `clear` results for selected
subjects, so it does not imply full contract verification. Findings remain available whatever decides the exit code.

File diagnostics highlight the contract requirement; Tenet does not invent an exact source span for a file-level judgment. The reviewing agent locates actionable code.
