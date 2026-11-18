# Qualify a local check

Tenet 0.18.0 can run a project checker against examples with known outcomes. Give it working code, a change that breaks the contract, and a change that preserves the behavior. The checker qualifies when it accepts both working versions and catches the defect. Qualification measures the examples you supply. Choose boundary cases that would matter to a caller.

## Declare controls

Start with a [local check](./local-checks.md). For the `options/preserve-zero` example, declare the checker in `.contracts/001-options/checks/preserve-zero.json`. Include `src/options.py`, the test and every project dependency the test reads in the binding's `inputs`. Install external dependencies in the execution environment.

Beside the binding, create `preserve-zero.qualification.json`.

```json
{
  "version": 1,
  "subjects": ["src/options.py"],
  "cases": [
    { "name": "existing behavior", "kind": "good" },
    { "name": "falls back on zero", "kind": "defect", "patch": "controls/fallback.patch" },
    { "name": "extracts a helper", "kind": "refactor", "patch": "controls/helper.patch" }
  ]
}
```

List exact, governed, project-relative implementation files in `subjects`. Each must be a declared checker input. Keep the test, checker, rules and configuration outside this list. The checker's project executable must use a relative path such as `./bin/check`. Commands run from the temporary project root. Use relative paths for project arguments and dependencies.

Each patch is a Git diff with project-relative paths, stored relative to the manifest. Create each diff against the same version of the working files. For example, the defect patch might change `return default if value is None else value` to `return value or default`. The refactor could extract the presence check into a helper and preserve every return value. A good control may also specify a patch, including when the current source needs repair to establish the known-good behavior. Every defect and refactor needs a patch.

Declare at least one case of each kind and give every case a distinct name and resulting tree. With the JSON checker protocol, add `"finding_case": "explicit zero"` to a defect when the checker must identify that particular counterexample. This assertion can expose checkers that fail for an unrelated reason.

## Run qualification

```sh
tenet check --qualify options/preserve-zero --json
```

Tenet stages the working bytes of the contract, binding, declared inputs and project executable. It applies each control independently in a new temporary checkout and executes the checker there with its normal timeout and output bounds. It rechecks the source dependencies and control files before returning. No model, saved local-check result or review revision is needed. Private checkouts use their own Git index and configuration. The checker is trusted project code with the caller's access to the machine, including external programs and absolute paths.

Each case reports the expected and observed checker outcomes, a message, the resulting Git tree ID, and the patch SHA-256 when applicable. A correctly detected defect has observed outcome `fail` and case outcome `pass`. A missing dependency, unreadable or inapplicable patch, or unresolved checker outcome leaves the case unresolved. The command accepts up to 64 cases, a 64 KiB manifest and 64 MiB of patches in total.

| Exit | Meaning                                                               |
| ---- | --------------------------------------------------------------------- |
| 0    | Every control produced the expected observation.                      |
| 1    | At least one decisive checker observation disagreed with its control. |
| 2    | The command or check selector was invalid.                            |
| 3    | Qualification was inconclusive.                                       |
| 4    | Source dependencies or controls changed while the checks ran.         |
