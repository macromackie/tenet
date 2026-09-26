# Changelog

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
