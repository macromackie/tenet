# Local checks

Attach an existing test or linter to one claim in a contract. Tenet can run it locally, keep its evidence, and tell you when the inputs have changed. This workflow needs the checker installed and Tenet 0.17.0 or later. It uses no model credentials.

## Attach one test

For a Python project with pytest installed and a test that checks an explicitly supplied zero, write the contract at `.contracts/001-options/CONTRACT.md`.

```markdown
---
name: options
message: Preserve explicitly supplied values.
requires-tenet: 0.17.0
paths: ['src/**/*.py']
checks: [preserve-zero]
---

## Rules

An explicitly supplied value takes precedence over the configured default, including zero.
```

Beside it, create `checks/preserve-zero.json`. The arguments select the project's existing test. Commands run from the project root.

```json
{
  "args": ["-m", "pytest", "-q", "tests/test_options.py::test_explicit_zero"],
  "inputs": ["src/**/*.py", "tests/test_options.py"],
  "program": "python3",
  "timeout_seconds": 30,
  "version": 1
}
```

```sh
tenet check --local options/preserve-zero
tenet check --local options/preserve-zero --fresh
tenet status --local --json
```

The first command reports the checker's diagnostic. Repair the implementation and use the second command to replay it.
The third inspects every declared check without executing commands or writing state. Omit the name to run all local checks.
Review a binding and its checker before executing it. The process runs with the caller's access and environment.
A local run, checker qualification, and a final check with local execution all run project code. For a final check on code supplied by someone else, run the checker in a separate environment and pass its receipt to Tenet. See [Final checks](./final-checks.md).

To test whether your checker can distinguish broken and equivalent implementations, see [Qualify a local check](./qualifying-checks.md).

## Checker results

The default `exit` protocol uses code 0 for a pass and 1 for an observed failure. Other exits and signals are unresolved.
Use a wrapper when a tool assigns those exit codes different meanings. Both output streams are captured.

A checker can set `"protocol": "json"` in its binding. It exits 0 after reporting its observation as one JSON object on stdout.
The required fields are `outcome` (pass, fail, or unresolved) and a nonempty `message`. A `finding` is optional.
When supplied, its `case`, `expected`, and `actual` fields are strings and `location` is an optional string.

```json
{
  "outcome": "fail",
  "message": "The default replaced an explicit zero.",
  "finding": {
    "case": "explicit zero",
    "expected": "0",
    "actual": "10",
    "location": "src/options.py:12"
  }
}
```

A tool that cannot start, malformed checker output, an exceeded limit, or inputs that change during the run leave the check unresolved. Other checks continue. When running several checks, Tenet compares their inputs again before reporting. If a later checker edits the input of an earlier passing check, that earlier result is stale. Each binding accepts a timeout of 1–600 seconds, up to 16 KiB from each output stream and up to 4,096 project input files totaling 64 MiB. The executable has a separate 256 MiB hash budget.

## Reuse and review

Set `"cache": true` only for reproducible checks. Include test helpers, rule files, tool configuration, and runtime dependency locks in `inputs`. Tenet matches each glob against project discovery from the root. Each pattern must match and at least one input must be governed by the contract. Tenet compares the contract, binding, resolved executable and its bytes, discovered paths, and file contents. A changed dependency makes the prior observation stale. Environment variables, installed plugins, external services, and transitive tools need separate control by the project. Leave caching off when those can change the result. A fresh run always executes the check. Unresolved observations are retried.

Local observations live under the review state directory. `tenet status` includes them after the first run. Its exit code continues to describe the semantic review. `tenet check --local` provides a separate gate for the selected executable claims. Exit 0 means all passed, 1 indicates a current failure, 2 indicates a command or state error, 3 indicates unresolved evidence, and 4 indicates missing or stale evidence or an empty selection of declarations. Pending evidence takes precedence over current failures, and failures take precedence over unresolved observations. A passing example establishes only the claim it exercises. For independently produced evidence, see [mechanical receipts](./cli.md#mechanical-evidence).

## Apply native fixes

Run `tenet fix` to apply the safe fixes a contract's installed Oxlint or Ruff configuration offers.
Select a contract, a path or the files changed since a Git commit. For example:

```sh
tenet fix --contract cleanup src/
tenet fix --base HEAD --json
```

Install the native tool on your command path. Tenet reads the adopted configuration recorded in
`.tenet.lock` at `assets/oxlint/oxlintrc.json` or `assets/ruff/ruff.toml`. Oxlint assets can name
built-in plugins and rules. Configurations that extend another Oxlint configuration, use custom
plugins or define overrides require a project-owned check. A local edit to the contract, including
a written exception, leaves that contract for ordinary review because the installed native rules
describe the upstream contract.

Tenet applies fixes to copies of the selected source files and reruns the selected linters on the
result. It prints the patch, replaces changed files in the working tree and lists findings that
still need a person or agent. A setup or execution problem leaves the patch unapplied. Contracts
without a supported native fixer appear separately. Run `tenet check` to review the resulting code.
