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
| unresolved | Evidence, coverage, confidence, or execution is insufficient |

A full check cannot conclude unaffected or preserved. A diff check cannot newly verify the baseline.
These are model judgments, not formal proofs. No findings alone is insufficient for verification.

File results remain `pass`, `fail`, `not_applicable`, `uncertain`, `error`, or `incomplete`.
The verbose reporter shows uncertain file judgments as `UNRESOLVED`. A repository assessment can resolve them when combined context supplies the missing evidence; the original file judgments remain in the report. Operational errors and exhausted budgets retain separate counts and exit codes.

## JSONL

Each event has `version: 1` and a `type`. The `begin` event records `full`, `diff`, or `eval` mode, provider/model, root, base and HEAD when available, and the baseline assumption. Snapshot runs include the original snapshot path and patch hash.

`result` contains file evidence: mode, change kind, previous path for renames, before/after hashes, contract hash, judgments, and timing. `source_hash` identifies the complete encoded change input.

`contract_finished` includes a `conclusion` with status, reason, optional repository assessment, evidence hash, and operational error. For example evaluation, this field is null; expected file judgments determine success.

Other events include `contract_started`, `check_started`, `stage_started`, `stage_completed`, `summary`, and `error`. Stages are `applicability`, `verification`, and `completeness`.
The summary includes file counts, contract counts, request count, and exit code. Source contents and credentials are not printed.

## Exit codes

| Code | Meaning |
| --- | --- |
| 0 | No contract failures; eval completed, with mismatches allowed unless `--strict` |
| 1 | Contract failures, or eval expectation mismatches with `--strict` |
| 2 | Invalid input or execution error |
| 3 | File checks could not complete, such as when the request budget is exhausted |
| 130 | Cancelled |

Unresolved contracts do not fail a check. They remain unresolved in the report; exit code 0 does not mean every contract was verified.
Execution errors and incomplete file checks still exit nonzero and take precedence over contract failures. Findings remain available in all cases.
For `eval`, mismatches exit 0 unless `--strict` is set. An explicitly expected uncertain judgment can match an example.
Fixture evaluation uses `eval_begin`, `case_begin`, `case_event`, `case_finished`, and `eval_summary` records; see [Evaluation](evaluation.md).

File diagnostics highlight the contract requirement; Tenet does not invent an exact source span for a file-level judgment. The reviewing agent locates actionable code.
