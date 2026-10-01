# Changelog

## v0.4.5

- Judge each file against the contract's stated scope, which may cover code, configuration, tests, or documentation.
  Notes that describe a product no longer fall outside a documentation contract.
- Send each resulting file to the model once. File-level inputs are 13–38% smaller on the evaluation fixtures.
- Use ev-grep 0.4.4, which retries a model response that fails validation, such as probabilities that do not sum to
  one, instead of ending the check with an execution error.
- Stop parsing `## Applies to` separately. The model reads it with the rest of the contract, and an empty section
  no longer fails validation.
- Document optional agent guidance, targeted questions with ast-grep and ev-grep, run-to-run variation, one concern
  per contract, and a JSONL gate that also fails on unresolved contracts.

## v0.4.4

- Ask relevance and violation questions together, with shared before/after context.
- Check independent contracts concurrently within one worker limit. Print final summaries in contract order.
- Keep unrelated source out of repository assessments and surface conflicting file assessments as unresolved.
- Include request attempts, hedges, timing, and available usage in JSON reports.

## v0.4.3

- Use ev-grep 0.4.0 for the shared provider adapter.

## v0.4.2

- Reserve context space for required source and JSON metadata before adding optional neighboring files. Small changes in larger repositories can now reach the final assessment without optional context exhausting the request limit.

## v0.4.1

- Avoid duplicating complete added files as patches in model inputs.


## v0.4.0

- Read whole contract documents, including exceptions and project check instructions.
- Report contract assessments with model confidence in JSONL v2.
- List scoped contracts as JSON and add related files with `check --context`.
- Combine source across files before deciding a contract failed.
- Evaluate repository fixtures with optional related-file context.


## v0.3.1

- Use `--endpoint` or `TENET_ENDPOINT` to run checks through a trusted provider proxy.

## v0.3.0

- `check --base REF` assesses before/after changes under a valid-base assumption. It replaces `--changed-since`. Full checks also assess repository context, including missing required files.
- `check --snapshot DIR --patch FILE` checks a copied repository without modifying the original.
- `eval FIXTURES` runs repositories and patches against expected contract conclusions. Use `--case` to select a case and `--validate` to check fixtures offline. Reports are saved under `.tenet/evals/`.
- Terminal output shows one result per contract with steady progress. Only failures include requirement snippets. Use `--reporter verbose` for file judgments.
- Unresolved contracts exit 0. Failures exit 1; execution errors and incomplete checks remain nonzero. Evaluation mismatches exit 0 unless `--strict` is set.

See [Evaluation](https://tenet-contracts.com/docs/evaluation) and [Output](https://tenet-contracts.com/docs/output) for the updated formats.
