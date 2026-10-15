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
content of every scoped file. A record from `tenet record` settles its input. Otherwise a model's `verified`,
`preserved`, `unaffected`, or `clear` settles it. Every other contract is pending:

| Reason | Meaning |
| --- | --- |
| `model_failed` | The model found a violation; confirm or dispute it with a record |
| `model_unresolved` | The model could not decide, or its evidence or execution was incomplete |
| `not_assessed` | `check --no-model` captured the input without asking the model |
| `inputs_changed` | A scoped file or the contract changed after the latest check (`status` only) |
| `not_checked` | The contract joined the review after the latest check (`status` only) |

A record or conclusion never carries to a different input. `tenet show` lists outdated ones with the changes since.

File results remain `pass`, `fail`, `not_applicable`, `uncertain`, `error`, or `incomplete`.
After confidence routing, a violation `match` produces `fail` even when relevance says `no_match`. Otherwise, relevance `no_match`
produces `not_applicable`; that status retains the source for the combined assessment. For other relevance outcomes,
violation `no_match` produces `pass` and uncertain violation produces `uncertain`. Raw answers and routing remain visible.

The verbose reporter shows uncertain file judgments as `UNRESOLVED`. A repository assessment can resolve them when combined context supplies the missing evidence; the original file judgments remain in the report. Operational errors and exhausted budgets retain separate counts and exit codes.

## JSONL

Each event has `version: 4` and a `type`. The `begin` event records `full` or `diff` mode, provider/model (`null` for `--no-model`), root, base and HEAD when available, the confidence threshold, and the baseline assumption. Snapshot runs include the original snapshot path and patch hash.

`result` contains file evidence: mode, change kind, previous path for renames, before/after hashes, contract hash, judgments, and timing. `source_hash` identifies the complete encoded change input. `input_bytes` measures the encoded state including contract and explicit context, before provider instructions and question wrappers; it is not a token count.

`contract_finished` includes the contract's `state`, such as `{"state": "pending", "reason": "model_failed"}` or
`{"state": "settled", "conclusion": "failed", "record": "d370e7dc32d8"}`, and the model's `conclusion`:

```json
{
  "assessment": "clear",
  "scope": "selected_subjects",
  "confidence": 0.95,
  "reason": "No violation found in the selected subjects; this does not verify the full contract scope.",
  "reason_code": "selected_subjects_clear",
  "request": null
}
```

Confidence is the model's score for its selected answer, not a measured probability that the contract holds.
The raw `choice`, `confidence`, and probabilities stay unchanged. `routing` separately records the outcome after
applying `--min-confidence` (default 0.8). Low-confidence and explicit uncertain answers route to unresolved.
File results retain the same distinction in `applicability_routing` and `verification_routing`. When there is no final
model assessment, confidence is null. Errors add an `error` field.

`reason` explains the conclusion, including selected-path coverage, unavailable context, model uncertainty, and
conflicting judgments. When a repository assessment ran, `model_assessment` preserves its answer and probabilities,
and `evidence_hash` identifies its encoded input without printing source. `conflicting_files` lists file failures
that contradict a favorable repository answer. Those disagreements remain unresolved.

`reason_code` supports routing without parsing prose:

| Code | Next step |
| --- | --- |
| `file_checks_incomplete` | Inspect file errors or incomplete results |
| `selected_subjects_clear` | Review the remaining scope before claiming full verification |
| `no_relevant_changes` | Retain the compliant-base assumption |
| `request_budget` | Account for unassessed work before increasing the budget |
| `context_too_large` | Use bounded evidence and manual review; do not silently omit required source |
| `context_unavailable` | Inspect the source-read error |
| `assessment_failed` | Inspect the provider error |
| `model_uncertain` | Investigate missing evidence and the raw assessment |
| `conflicting_assessments` | Trace the flagged files against the combined evidence |
| `violation` / `compliance` | Review the model's conclusion and evidence |

When combined context was assembled, `context` reports `input_bytes`, `limit_bytes`, `included_files`,
`selected_files`, `support_files`, and `omitted_files` (paths only). All captured selected source in the contract scope
is retained regardless of relevance answers. Omitted paths come from inventory outside the supplied source. These
fields describe the supplied state, not proof that it contains every necessary implementation.
The same diagnostics and evidence hash remain available if the provider fails. Oversize reasons give measured bytes;
source is never truncated to fit.
Inventory metadata may use [compact directory trees](./limits.md); scope diagnostics retain expanded paths, while
`input_bytes` and `evidence_hash` describe the actual encoded assessment input.

Version 4 adds each contract's `state`, the summary's `revision` and `pending` contracts, and `cached` counts for
results answered from earlier identical requests. Version 3 replaced the run-wide `partial` flag with each conclusion's
`scope` and separated raw answers from routing. Consumers should retain useful diagnostics and ignore unknown fields. A reason describes the evidence limit; it does not turn an unresolved assessment into a pass.

Other events include `contract_started`, `check_started`, `stage_started`, `stage_completed`, `summary`, and `error`. Stages are `applicability`, `verification`, and `completeness`.
The summary includes file counts, contract counts, request and cached counts, the appended `revision` (`null` for a snapshot), the `pending` contract names, and the exit code. Source contents and credentials are not printed.

## Check exit codes

`check` and `status` share these codes:

| Code | Meaning |
| --- | --- |
| 0 | Every contract is settled without a violation |
| 1 | A settled violation: a recorded failure |
| 2 | Invalid input or execution error |
| 3 | A recorded `unresolved` conclusion |
| 4 | A contract is pending, including model failures, unresolved answers, and exhausted budgets |
| 130 | Cancelled |

Cancellation takes precedence, then execution errors, pending contracts, violations, and recorded unresolved
conclusions. A model's `failed` exits 4 until a record confirms it. Exit 0 can include `clear` results for selected
subjects, so it does not imply full contract verification. Findings remain available whatever decides the exit code.

File diagnostics highlight the contract requirement; Tenet does not invent an exact source span for a file-level judgment. The reviewing agent locates actionable code.
