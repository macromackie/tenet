# Output

The default reporter shows progress while files are assessed, then one final result per contract. Unresolved results show only their status; operational errors remain visible. Only failed contracts show requirement snippets and finding details. Interactive terminals show a spinner, file counts, and elapsed time below completed contracts. Failure details follow the results. Redirected output contains no animation.

Use `--reporter verbose` for individual file judgments and unresolved reasons. JSONL retains all intermediate evidence, including uncertainty that a later repository assessment resolves.
Human reports go to stderr. JSONL goes to stdout.

```sh
tenet check --base origin/main --reporter jsonl > report.jsonl
```

## Contract conclusions

| Conclusion | Meaning |
| --- | --- |
| verified | Full-check evidence supports compliance |
| preserved | Relevant changes preserve compliance, assuming a valid base |
| unaffected | No relevant changes found, assuming a valid base |
| failed | Evidence supports a violation |
| unresolved | The model cannot decide, or required evidence or execution is incomplete |

A full check cannot conclude unaffected or preserved. A diff check cannot newly verify the baseline.
These are model judgments, not formal proofs. No findings alone is insufficient for verification.

File results remain `pass`, `fail`, `not_applicable`, `uncertain`, `error`, or `incomplete`.
The verbose reporter shows uncertain file judgments as `UNRESOLVED`. A repository assessment can resolve them when combined context supplies the missing evidence; the original file judgments remain in the report. Operational errors and exhausted budgets retain separate counts and exit codes.

## JSONL

Each event has `version: 2` and a `type`. The `begin` event records `full` or `diff` mode, provider/model, root, base and HEAD when available, and the baseline assumption. Snapshot runs include the original snapshot path and patch hash.

`result` contains file evidence: mode, change kind, previous path for renames, before/after hashes, contract hash, judgments, and timing. `source_hash` identifies the complete encoded change input. `input_bytes` measures the encoded state including contract and explicit context, before provider instructions and question wrappers; it is not a token count.

`contract_finished` includes a `conclusion`:

```json
{
  "assessment": "unresolved",
  "confidence": null,
  "reason": "Only selected paths were checked; contract-wide coverage is incomplete.",
  "reason_code": "partial_scope",
  "request": null
}
```

Confidence is the model's score for its selected answer, not a measured probability that the contract holds.
A low score does not change the answer to unresolved. An explicit uncertain answer does. When there is no final
model assessment, confidence is null. Errors add an `error` field.

`reason` explains the conclusion, including selected-path coverage, unavailable context, model uncertainty, and
conflicting judgments. When a repository assessment ran, `model_assessment` preserves its answer and probabilities,
and `evidence_hash` identifies its encoded input without printing source. `conflicting_files` lists file failures
that contradict a favorable repository answer. Those disagreements remain unresolved.

`reason_code` supports routing without parsing prose:

| Code | Next step |
| --- | --- |
| `file_checks_incomplete` | Inspect file errors or incomplete results |
| `partial_scope` | Review the remaining scope |
| `no_relevant_changes` | Retain the compliant-base assumption |
| `request_budget` | Account for unassessed work before increasing the budget |
| `context_too_large` | Use bounded evidence and manual review; do not silently omit required source |
| `context_unavailable` | Inspect the source-read error |
| `assessment_failed` | Inspect the provider error |
| `model_uncertain` | Investigate missing evidence and the raw assessment |
| `conflicting_assessments` | Trace the flagged files against the combined evidence |
| `violation` / `compliance` | Review the model's conclusion and evidence |

When repository context was assembled, `context` reports `input_bytes`, `limit_bytes`, `included_files`, and
`omitted_files` (paths only). These describe the supplied state, not proof that it contains every necessary implementation.
The same diagnostics and evidence hash remain available if the provider fails. Oversize reasons give measured bytes;
the required-source subtotal can exceed the limit before the complete state is assembled.

These diagnostic fields are additive within JSONL version 2. Consumers should retain useful diagnostics and ignore
unknown fields. A reason describes the evidence limit; it does not turn an unresolved assessment into a pass.

Other events include `contract_started`, `check_started`, `stage_started`, `stage_completed`, `summary`, and `error`. Stages are `applicability`, `verification`, and `completeness`.
The summary includes file counts, contract counts, request count, and exit code. Source contents and credentials are not printed.

## Exit codes

| Code | Meaning |
| --- | --- |
| 0 | No contract failures |
| 1 | Contract failures |
| 2 | Invalid input or execution error |
| 3 | File checks could not complete, such as when the request budget is exhausted |
| 130 | Cancelled |

Unresolved contracts do not fail a check. They remain unresolved in the report; exit code 0 does not mean every contract was verified.
To also fail when any contract is unresolved, read the JSONL summary. The pipe hides Tenet's own exit status, so
check it there too:

```sh
tenet check --json | jq -e 'select(.type == "summary") | .exit_code == 0 and .summary.contracts_unresolved == 0'
```
Execution errors and incomplete file checks still exit nonzero and take precedence over contract failures. Findings remain available in all cases.

File diagnostics highlight the contract requirement; Tenet does not invent an exact source span for a file-level judgment. The reviewing agent locates actionable code.
