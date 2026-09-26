# Evaluation

`check` assesses code. `eval` compares Tenet's answers with expectations you wrote.
A correctly detected violation makes an evaluation match.

## Repository fixtures

```text
fixtures/cache/
  .contracts/001-cache-values/CONTRACT.md
  base/src/cache.ts
  patches/broken-cache-hit.patch
  eval.toml
```

```toml
[[cases]]
name = "baseline"

[cases.expect]
cache-values = "verified"

[[cases]]
name = "broken-cache-hit"
patch = "patches/broken-cache-hit.patch"

[cases.expect]
cache-values = "failed"
```

Save the TOML above as `fixtures/cache/eval.toml`. The contract and base files are ordinary project files; create the patch with `git diff`.

```sh
tenet eval fixtures/cache --validate
tenet eval fixtures/cache
tenet eval fixtures/cache --case broken-cache-hit
tenet eval fixtures
```

Each case checks a fresh copy of `base/` with its sibling `.contracts/`. Without a patch, it runs a full check.
With a patch, it runs a diff check assuming the base satisfies the contracts. Patches never accumulate.
Expected answers stay outside the repository sent to the model.

Name every selected contract in `expect`. Set `contract = "cache-values"` on a case to check just that contract.
Full checks expect `verified`, `failed`, or `unresolved`. Diff checks expect `preserved`, `unaffected`, `failed`, or `unresolved`.
Use `--contract NAME` to narrow a run further.

Cases default to `split = "development"`. Mark held-out cases with `split = "heldout"` and run them with
`--split heldout`; `--split all` runs both. `--validate` checks the selected cases and applies their patches without model calls.

## Results

Illustrative output:

```text
 ✓ cache/baseline (1/1 expectations matched) 0.8s
 ? cache/broken-cache-hit (0/1 expectations matched) 0.6s

 cache/broken-cache-hit
   cache-values: expected failed, received unresolved

 Cases         1 matched | 1 mismatched | 0 errors (2)
 Expectations  1 matched | 1 mismatched
```

Mismatches exit 0 by default. Add `--strict` to exit 1 when any expectation differs.
Execution errors exit nonzero and never count as matches. Unresolved is a match only when it was expected.

```sh
tenet eval fixtures --strict --output results/run.jsonl
tenet eval fixtures --json
```

Fixture runs save a JSONL report under `.tenet/evals/` unless `--output` names another file.
Reports include expected and actual conclusions, model and fixture identities, and intermediate engine events.
`--json` also streams these records to stdout. Existing report files are not overwritten.

Cases run in order; file checks use `--jobs`. `--max-requests` limits the entire fixture run.
Provider and model options are the same as `check`.

## Inline examples

Small examples inside contracts remain useful for testing individual file judgments:

```sh
tenet eval --contract database-failures
tenet eval --contract database-failures --strict
```

Without a fixture path, `eval` runs inline examples through relevance and verification.
It does not perform repository assessment. Regular examples use full-mode file judgments;
`tenet:change` examples use diff-mode file judgments. Expected labels never enter the prompt.

Keep direct violations, allowed exceptions, unrelated code, and missing context in the fixture set.
Do not change an expectation just to match a model response.

Evaluation makes live provider calls. Ordinary Rust tests run offline with a deterministic evaluator.
Passing software tests verifies program behavior; it does not establish model accuracy.
