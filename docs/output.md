# Output

The default reporter groups file checks under each contract. Interactive terminals show progress; CI and redirected output use stable lines.
Failures and unresolved checks include the requirement excerpt. `--reporter verbose` prints every check.
Human reports go to stderr. JSONL events go to stdout without terminal decoration.

```sh
tenet check --reporter jsonl > report.jsonl
```

Every JSON line has `version: 1` and a `type`. Events include `begin`, `contract_started`, `check_started`,
`stage_started`, `stage_completed`, `result`, `contract_finished`, `skipped`, `summary`, and `error`.
A `result` contains a `result` object with contract and source hashes, paths, status, assessments, and duration.
Example results also contain `example` and `expected`. The summary contains counts and the exit code.
The beginning identifies the provider, model, project root, HEAD, and comparison base when available.

Statuses are `pass`, `fail`, `not_applicable`, `uncertain`, `error`, and `incomplete`.
Excluded files never increase the pass count. A contract with no checked files is displayed as `EMPTY`.
A run with no candidate pairs may complete successfully; its zero counts do not establish coverage.

## Exit codes

| Code | `check` |
| --- | --- |
| 0 | Completed without failures, uncertainty, or errors |
| 1 | Contract failures |
| 2 | Invalid input or execution error |
| 3 | Uncertainty or incomplete checks |
| 130 | Cancelled |

Errors take precedence over uncertainty, which takes precedence over failures. Findings remain in the report in all cases.
For `eval`, exit 1 means an expectation mismatch. An expected uncertain judgment can pass an example.
Errors, incomplete work, and cancellation retain their exit codes.

A file-level failure supplies no exact source location. The diagnostic highlights the contract requirement, not an invented code span.
A reviewing agent should inspect the file and establish an actionable code location.
