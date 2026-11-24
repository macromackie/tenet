# Tenet

Tenet checks code against requirements written in Markdown under `.contracts/`.

## Install

```sh
curl -fsSL https://tenet-contracts.com/install.sh | bash
```

Binaries support macOS and Linux on ARM64 and x86-64. Local checks run with tools installed in your project. For model review, set `OPENROUTER_API_KEY` or configure TypeSafe. See [Provider setup](docs/installation.md).

## Quickstart

Verify your environment, scan the repository into `.contracts/`, check code against contracts, repair findings, and inspect status:

```sh
tenet doctor                       # Verify credentials, sandbox tooling, and repository setup
tenet scan                         # Deep-scan modules by default (pass --shallow for a faster scan)
tenet scan --apply                 # Write contracts and runnable scripts into .contracts/
tenet scan --ephemeral             # Materialize a trial scan into .tenet/scan/.contracts/
tenet check                        # Run semantic review, static checks, and locked runtime scripts
tenet check --base origin/main     # Review a pull request diff against the base branch
tenet fix                          # Apply native lint fixes and sandboxed runtime repairs
tenet fix --severity high          # Limit repairs to findings at or above one severity level
tenet status                       # Show repository coverage, persisted check receipts, and contract health
tenet show timeouts                # Inspect the rule, diff, evidence, and runnable script path
```

With `OPENROUTER_API_KEY` set on macOS or Linux, Tenet builds qualified behavior checks in an OS sandbox, streams live progress phases in interactive terminals, and explains failed or unresolved findings through `fx`. As an explicit fallback for air-gapped environments, pre-commit hooks, or CI gates without provider credentials, `tenet check --skip-semantic` (compatibility aliases `--offline`, `--replay`) skips semantic contract review and only replays deterministic static checks, declared local checks, and locked runtime scripts in `scripts/`, saving its receipt to `.tenet/offline-check.json` for `tenet status`. `tenet init` and `tenet audit` remain supported as backwards-compatible aliases for `tenet scan`. See [behavior checks](docs/behavior-checks.md), [local checks](docs/local-checks.md), and [final checks](docs/final-checks.md).

## Contracts

Store each contract as a self-contained folder under `.contracts/`:

```text
.contracts/
  001-database-failures/
    CONTRACT.md
    scripts/
      preserve-errors.mjs
      .lock.json
```

- **Portable specification:** `CONTRACT.md` holds the human- and agent-readable rule, scope, and examples.
- **Runnable verification:** `scripts/` stores materialized behavior-check scripts (`<check>.<ext>`) beside `scripts/.lock.json`.
- **Hand-edit safety:** `scripts/.lock.json` tracks script SHA-256 digests and a compact `controlHash` so manual edits to any script are preserved and never overwritten during regeneration.
- **Format compatibility:** Catalog contracts store tool configurations under `assets/`, and Tenet continues to read legacy `agent-checks.lock.json` files beside `CONTRACT.md`.

Install shared contracts from the public [tenet-contracts](https://github.com/macromackie/tenet-contracts) library:

```sh
tenet contracts recommend
tenet contracts add tenet/data-and-apis --scope packages/api
tenet contracts update
```

See [Contracts](docs/contracts.md) and [Guidance](docs/guidance.md) for scope patterns, local adaptations, and updates.

## Review changes

```sh
tenet check --base origin/main
tenet status
tenet show database-failures
tenet record database-failures --conclusion failed --evidence src/query.ts:40-52 -m "returns [] on failure"
```

Tenet checks relevant files and their combined context, then reports one conclusion per contract. A model's `failed` or `unresolved` conclusion stays pending, with exit 4, until a reviewer records a conclusion bound to the exact code it judged. Editing a scoped file marks earlier records outdated. Selected code and contract rules go to your configured provider.

- **[Reviews](docs/reviews.md):** Settle pending contracts with repository evidence and adopt requirements incrementally.
- **[Export](docs/export.md):** Export selected contracts and source offline for a review or focused question.
- **[Commands](docs/cli.md):** Inspect all flags, including `--contract-batch-size`, `--context-profile long`, and `--dry-run --json`.
- **[Output](docs/output.md) and [Limits](docs/limits.md):** Read exit codes, coverage blocks, JSONL schemas, and byte bounds.

Browse the full documentation at [tenet-contracts.com/docs](https://tenet-contracts.com/docs).

## Build

```sh
cargo build --release --locked -p tenet
cargo install --path crates/tenet --locked
```

The Rust binary embeds the checker worker. To change the worker, use Node 22.23.2 and regenerate the bundle before building:

```sh
cd checker
npm ci --ignore-scripts
npm run typecheck
npm test
npm run bundle
```

CI rebuilds the worker and compares it with the committed copy. A fresh checkout replays locked scripts using Node and the OS sandbox tools for the target language. The first model-generated check installs the pinned `fx` library in the user cache.

## Package boundaries

[package-boundaries.json](package-boundaries.json) declares package ownership and allowed crate dependencies. CI verifies that declaration against Cargo and source imports alongside source-copy and advisory checks.

Run the package check from a source checkout with Cargo, Python 3.14 or later, and `uv`:

```sh
shasum -a 256 --check .checks/package-boundaries/SHA256SUMS
uv run --locked --script .checks/package-boundaries/check-package-boundaries.py --root . --config package-boundaries.json
```

Policies that include JavaScript or TypeScript packages also install the npm resolver described in the bundled guide.

[MIT](LICENSE).
