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
| `clear` for selected subjects | `clear` for selected changes |
| `failed` | `failed` |
| `unresolved` | `unresolved` |

Failures include the requirement and evidence. A `clear` result applies only to the requested selection. These are model judgments, so a model's `failed` or `unresolved` result stays pending until a reviewer records a conclusion. See [Output](./output.md).

## Try a shared contract

```sh
tenet contracts add tenet/code-comment-attribution --scope .
tenet check --dry-run
tenet check
```

Install a contract from the [shared library](./guidance.md) into the folder it governs, list the requests a check
would make, then run it. To compare code that follows a contract with a change that breaks it, see [Evaluation](./evaluation.md).

## Use in a review

```sh
tenet check --base origin/main
tenet status
tenet show database-failures
tenet record database-failures --conclusion failed --evidence src/storage/query.ts:40-52 -m "returns [] when the query fails"
```

`check` keeps each result in `.tenet/`, keyed by the exact code it assessed. A [reviewing agent](./reviews.md) settles
each pending contract by recording a conclusion with evidence. Editing a scoped file makes the record outdated. Tenet uses
Jev through OpenRouter or TypeSafe; selected code and contract rules go to that provider.

Use [`tenet export`](./export.md) to export selected contracts and source offline for a review or a focused question.
