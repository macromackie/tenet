# Reviews

Tenet supplies fast, model-based checks. The reviewing agent reads the contracts, investigates the code,
runs appropriate project checks, and writes the final review.

```sh
tenet contracts list --base origin/main --json
tenet contracts view database-failures
tenet check --base origin/main --json > report.jsonl
```

Read every contract in the inventory. Use failed, unresolved, and lower-confidence assessments to decide where
to investigate first. A favorable assessment can save time; it does not remove the contract from the review.
Confirm suspected violations before commenting on code. Exit 0 means no final failures, not complete verification.

## Add context

If a helper owns the error handling, supply it and rerun the relevant contract:

```sh
tenet check --base origin/main --contract database-failures \
  --context src/storage/query.ts --json
```

Context files supplement the input; they do not expand the contract's scope. Tenet retains their before/after identity.
Keep the checkout stable during a run. Capture stdout and exit status even when the command returns nonzero.

## Establish the baseline

A diff check assumes the base satisfies its contracts. On first adoption, run a full check. New or changed contracts
also need a full check of their scope:

```sh
tenet check --json
tenet check --contract database-failures --json
```

`contracts list --base ... --json` includes changed contract paths, including deleted ones. Inspect those changes;
a removed or weakened requirement must not silently disappear from the review.

The outer agent owns baseline validity, follow-up, and concerns outside the contracts. Tenet has no internal agent loop
or automatic escalation to a larger model.
