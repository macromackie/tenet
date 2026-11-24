# Tenet

Tenet checks code against requirements stored in `.contracts/`. Each contract folder pairs a Markdown specification (`CONTRACT.md`) with materialized runnable scripts (`scripts/` and `scripts/.lock.json`) or catalog tool configuration (`assets/`).

[Install Tenet](./installation.md), set your provider key, then run the core loop: `tenet scan`, `tenet check`, `tenet fix`, and `tenet status` / `tenet show`.

## Scan a repository

```sh
tenet doctor
tenet scan
tenet scan --apply
tenet scan --ephemeral
```

`tenet scan` inspects production modules, recommends catalog contracts, and runs deep adversarial edge-case synthesis by default to build per-module behavioral contracts that Tenet falsifies inside a local OS sandbox. Pass `--shallow` for a lighter, faster scan without adversarial edge-case synthesis. Pass `--apply` (or `--yes`) to write contracts and runnable scripts into `.contracts/`, or `--ephemeral` to materialize them into `.tenet/scan/.contracts/` without touching tracked repository files. `tenet init` and `tenet audit` remain supported as compatibility aliases.

## Check the repository or a change

```sh
tenet check
tenet check --base origin/main
```

- `tenet check` runs semantic review, static checks, and locked runtime scripts across the repository without assuming existing code complies.
- `tenet check --base origin/main` compares staged, unstaged, and untracked changes against the base revision, assuming the base complies.

As an explicit fallback for air-gapped environments, pre-commit hooks, or CI gates without provider credentials, `tenet check --skip-semantic` skips semantic contract review and only replays deterministic static checks, declared local checks, and locked runtime scripts in `scripts/`, saving its receipt to `.tenet/offline-check.json` (`--offline` and `--replay` remain supported as compatibility aliases).

Each contract finishes with one semantic conclusion:

| Full check                    | Diff check                   |
| ----------------------------- | ---------------------------- |
| `verified`                    | `preserved` or `unaffected`  |
| `clear` for selected subjects | `clear` for selected changes |
| `failed`                      | `failed`                     |
| `unresolved`                  | `unresolved`                 |

Failures include the requirement and evidence. A `clear` result applies only to the requested selection. Because model judgments can err, a model's `failed` or `unresolved` result stays pending until a reviewer records a conclusion. See [Output](./output.md).

## Fix findings

```sh
tenet fix
tenet fix --severity high
```

`tenet fix` applies safe native lint fixes from adopted contracts and repairs failing runtime scripts or witnesses in a bounded, sandboxed patch-and-reverify loop. Filter runtime repairs with `--severity`, or target one rule with `--contract NAME`. See [Behavior checks](./behavior-checks.md) and [Local checks](./local-checks.md).

## Inspect status and review changes

```sh
tenet status
tenet show database-failures
tenet record database-failures --conclusion failed --evidence src/storage/query.ts:40-52 -m "returns [] when the query fails"
```

`tenet status` prints a repository `Coverage` block, any persisted offline check receipt from `.tenet/offline-check.json`, and settled or pending contract health without calling a model. `tenet show NAME` prints the rule, diff, model answer, history, and direct path to any runnable script under `.contracts/<NNN>-<name>/scripts/`.

`tenet check` stores review state in `.tenet/`, keyed by the exact code it assessed. A [reviewing agent](./reviews.md) settles each pending contract by recording a conclusion with evidence. Editing a scoped file marks earlier records outdated. Model checks send selected code and contract rules to Jev through OpenRouter or TypeSafe.

## Shared contracts and automation

```sh
tenet contracts add tenet/code-comment-attribution --scope .
tenet check --dry-run
tenet export src/ --base origin/main > evidence.json
```

Install contracts from the [shared library](./guidance.md) into the directory they govern, preview planned requests with `--dry-run`, or export contracts and source offline with [`tenet export`](./export.md). Compare compliant code against a breaking change in [Evaluation](./evaluation.md), and parse any command's output as versioned [JSON](./json.md).
