# Commands

```sh
tenet check
tenet check --base origin/main
tenet eval
tenet validate
tenet contracts list [--for PATH] [--base REF] [--json]
tenet contracts view NAME
```

Use `--root DIR` to choose a project; the default is the working directory. Contracts outside that root do not participate.

## Full check

```sh
tenet check
tenet check --contract cache-values
```

Assess the current repository without assuming that any contract holds. File judgments are followed by a bounded repository assessment to check missing files and requirements that span files.

`tenet check src/` limits file checks to those paths. It can find failures, but reports unresolved contract coverage rather than verifying the entire contract.

## Diff check

```sh
tenet check --base origin/main
tenet check --base HEAD --dry-run --json
```

Assume the base satisfies the selected contracts, then assess whether the change preserves them. `--base REF` compares that exact commit with the working tree, including staged, unstaged, and untracked files. It does not choose a merge base automatically.

Each input includes before/after contents and the change kind. Deletions and renames are checked too. An unrelated change can leave a contract unaffected; relevant changes need evidence of preservation.
The caller owns establishing the baseline and running full checks when contracts change. Tenet does not silently switch modes.

`contracts list --base REF --json` returns the containing contract scopes in evaluation order, plus changed contract paths.
It needs no model credentials. New and old paths participate, including deletions and moves.

## Snapshot and patch

```sh
tenet check --snapshot ./base
tenet check --snapshot ./base --patch ./diff/get-or-set.patch
```

Copy the snapshot into a temporary repository, optionally apply a Git-compatible patch, and assess the result. The original directory stays unchanged. Temporary files are removed on exit.
If the snapshot has no `.contracts`, Tenet copies `.contracts` from its parent directory. Paths are relative to the snapshot's root; the patch uses ordinary `a/` and `b/` prefixes.

Without a patch this is a full check. With a patch it is a diff check. `--base` and `--snapshot` cannot be combined.

## Options

| Option | Default | Purpose |
| --- | --- | --- |
| `--contract NAME` | all contracts | Select one requirement |
| `--context PATH` | none | Add a repository file to check inputs; repeat for multiple files |
| `--jobs N` | 4 | Maximum simultaneous model requests |
| `--max-requests N` | 1000 | Request budget, including repository assessments |
| `--provider NAME` | openrouter | Select provider |
| `--model ID` | provider default | Select a supported pinned model |
| `--reporter KIND` | default | `default`, `verbose`, or `jsonl` |
| `--json` | off | Shorthand for JSONL |

`check --dry-run` lists candidate pairs without judging relevance or making model requests. `validate` and `contracts` also work without credentials.

## Discovery

The inventory respects Git ignore files and `.ignore`. Tool configuration and other hidden files are included. `.git`, `.context`, `.agents`, `.tenet`, `.env`, `.env.*`, `target`, `node_modules`, `dist`, and `out` are excluded. Contract support files are not code subjects.
Tracked changes to ordinary files are assessed even if current ignore rules hide them from the inventory. Never store credentials in files you submit for assessment.

Symlinks are not followed. Selected symlinks, binary files, unreadable inputs, and oversized inputs do not count as passing checks.
There is no configuration file, baseline database, generated verifier, or automatic fixer.

## Fixture evaluations

```sh
tenet eval fixtures/cache --validate
tenet eval fixtures/cache --case broken-cache-hit
tenet eval fixtures --split all --strict
```

See [Evaluation](evaluation.md) for the fixture format and saved reports.
