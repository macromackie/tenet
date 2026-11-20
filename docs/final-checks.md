# Final checks

Run a final check from the working tree after your last edit. Tenet assesses the current source, runs declared local checks and generated behavior checks, and reports all results in one document.

```sh
tenet check --final --json
```

For example, a contract can require a parser to preserve explicit empty arguments. If the parser changes after its test passed, Tenet compares the test's declared inputs before reusing its result. If the parser changes again during final verification, inspect the stale or pending row and replay the check on the finished edit.

Use the same scope as the change you intend to submit. A whole-project check assesses the selected contracts across the repository. A diff check such as `tenet check --final --base origin/main` assumes a compliant base. Each result carries its assessed scope.

## Credentials and project commands

The default final check runs trusted project commands with the caller's access. When another party controls the source or checker, run the checker in a separate environment and write its receipt outside that checkout. Then run the following command on the source you plan to submit.

```sh
tenet check --final --evidence /path/to/receipt.json --json
```

Tenet compares the receipt with the contracts, file inventory and bytes in the checkout. You choose and trust the producer of the receipt. Include the checker's code, configuration and other dependencies among the inputs it attests. See [Mechanical evidence](./cli.md#mechanical-evidence).

Use `--max-requests N` to bound new model requests (`--max-requests 0` reuses cached answers without sending new requests and leaves uncached inputs pending). Local and behavior checks still report their evidence.

## Read the result

Read `contracts` for semantic states, `checks` for each declared test or linter, and `exit_code` for the combined result. A current passing test establishes the behavior it exercised. A semantic conclusion can still require investigation. A missing or stale receipt, an input that could only be partly assessed, or a checker that could not run requires follow-up. Use the reported replay command after fixing the code or supplying the missing evidence.
