# Tenet

Tenet checks code against requirements in `.contracts/`. A contract might require database errors to reach the caller, cache hits to preserve stored values, or an index file to contain only exports.

[Install Tenet](./installation.md), set your provider key, then [write a contract](./contracts.md).

## Check the repository

```sh
tenet validate
tenet check
```

A full check examines files and their combined context. It makes no assumption that the repository already follows your contracts.

## Check a change

```sh
tenet check --base origin/main
```

A diff check assumes the base follows the contracts. It asks whether staged, unstaged, and untracked changes preserve them. Use a full check when adopting a new contract.

Each contract finishes with one result:

| Full check | Diff check |
| --- | --- |
| `verified` | `preserved` or `unaffected` |
| `failed` | `failed` |
| `unresolved` | `unresolved` |

Failures include the requirement and evidence. Unresolved results need follow-up; they do not fail the command. These are model judgments, so inspect findings before acting on them. See [Output](./output.md).

## Test a contract

```sh
tenet eval --contract database-failures
tenet eval fixtures/cache --case broken-cache-hit --strict
```

`eval` compares judgments with expected answers. It runs either examples inside contracts or [repository fixtures](./evaluation.md) containing a base directory, a patch, and expectations.

## Use in a review

```sh
tenet check --base origin/main --json > review.jsonl
```

A [reviewing agent](./reviews.md) reads the report, investigates failures and unresolved contracts, and writes the review. Tenet uses Jev through OpenRouter or TypeSafe; selected code and contract rules go to that provider.
