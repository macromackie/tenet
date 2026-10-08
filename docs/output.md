# Output

The default reporter shows progress while files are assessed, then one final result per contract. Unresolved results show the reason they could not complete. Only failed contracts show requirement snippets and finding details. Interactive terminals show a spinner, file counts, and elapsed time below completed contracts. Failure details follow the results. Redirected output contains no animation.

Use `--reporter verbose` for individual file judgments. JSONL retains all intermediate evidence, including uncertainty that a later repository assessment resolves.
Human reports go to stderr. JSONL goes to stdout.
The offline [`evidence` command](./evidence.md) has a separate JSON format containing source and contract documents.

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

File results remain `pass`, `fail`, `not_applicable`, `uncertain`, `error`, or `incomplete`.
The verbose reporter shows uncertain file judgments as `UNRESOLVED`. A repository assessment can resolve them when combined context supplies the missing evidence; the original file judgments remain in the report. Operational errors and exhausted budgets retain separate counts and exit codes.

## JSONL

Each event has `version: 3` and a `type`. The `begin` event records `full` or `diff` mode, provider/model, root, base and HEAD when available, the confidence threshold, and the baseline assumption. Snapshot runs include the original snapshot path and patch hash.

`result` contains file evidence: mode, change kind, previous path for renames, before/after hashes, contract hash, judgments, and timing. `source_hash` identifies the complete encoded change input. `input_bytes` measures the encoded state including contract and explicit context, before provider instructions and question wrappers; it is not a token count.

`contract_finished` includes a `conclusion`:

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
`selected_files`, `support_files`, and `omitted_files` (paths only). These describe the supplied state, not proof that it contains every necessary implementation.
The same diagnostics and evidence hash remain available if the provider fails. Oversize reasons give measured bytes;
source is never truncated to fit.

Version 3 replaces the run-wide `partial` flag with each conclusion's `scope` (`full_contract` or `selected_subjects`)
and separates raw answers from routing. Consumers should retain useful diagnostics and ignore unknown fields. A reason describes the evidence limit; it does not turn an unresolved assessment into a pass.

Other events include `contract_started`, `check_started`, `stage_started`, `stage_completed`, `summary`, and `error`. Stages are `applicability`, `verification`, and `completeness`.
The summary includes file counts, contract counts, request count, and exit code. Source contents and credentials are not printed.

## Check exit codes

| Code | Meaning |
| --- | --- |
| 0 | Every requested claim completed without a violation |
| 1 | Contract failures |
| 2 | Invalid input or execution error |
| 3 | Unresolved semantics or incomplete requested work, including exhausted budgets |
| 130 | Cancelled |

Unresolved contracts exit 3. Exit 0 can include `clear` results for selected subjects, so it does not imply full
contract verification. Cancellation takes precedence, then execution errors, unresolved or incomplete work, and
confirmed violations. Findings remain available even when another contract causes exit 2 or 3.

File diagnostics highlight the contract requirement; Tenet does not invent an exact source span for a file-level judgment. The reviewing agent locates actionable code.
