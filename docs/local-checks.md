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
tenet check --final --contract options
tenet check --final --contract options --fresh
tenet status --json
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

Local observations live under the review state directory. `tenet status` includes them after the first run. Its exit code continues to describe the review. Run `tenet check --final` (with model credentials configured) to execute semantic review and declared project checks together, or use the explicit fallback `tenet check --skip-semantic` (compatibility aliases `--offline`, `--replay`) when provider credentials are unavailable to skip semantic contract review and only replay declared local checks alongside static checks and locked runtime scripts. A passing example establishes only the claim it exercises. For independently produced evidence, see [mechanical receipts](./cli.md#mechanical-evidence).

## Apply fixes

Run `tenet fix` to repair both native lint violations and failing runtime behavior checks in one command:

```sh
tenet fix
tenet fix --contract cleanup src/
tenet fix --severity high --max-requests 20
tenet fix --base HEAD --json
```

`tenet fix` combines two repair paths without requiring `--from-audit`:

- **Native lint fixes:** When an adopted contract ships an Oxlint or Ruff configuration recorded in `.tenet.lock` (`assets/oxlint/oxlintrc.json` or `assets/ruff/ruff.toml`), Tenet runs the installed tool on copies of the selected files, verifies the edits with the linter, prints the patch, and writes changed files back to the working tree. Oxlint assets can name built-in plugins and rules; configurations that extend another Oxlint config, use custom plugins, or define overrides require a project-owned check. A local edit to the contract, including a written exception, leaves that contract for ordinary review because the installed native rules describe the upstream release.
- **Sandboxed runtime repairs:** When locked runtime scripts under `.contracts/<NNN>-<name>/scripts/` or recorded audit witnesses fail, Tenet runs a bounded patch-and-reverify loop inside the local OS sandbox until the failing scripts pass or the request budget runs out. Use `--severity` to restrict runtime repairs to findings at or above one severity level, and `--max-requests N` to bound model calls. `--from-audit` remains supported as a compatibility flag.

A setup or execution error leaves the working tree untouched. Findings that still require a person or reviewing agent appear in the output. Run `tenet check` for a full semantic and runtime review, or use the explicit fallback `tenet check --skip-semantic` (compatibility aliases `--offline`, `--replay`) to skip semantic contract review and only replay static checks, declared local checks, and locked runtime scripts.
