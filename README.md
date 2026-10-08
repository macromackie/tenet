# Tenet

Check code against requirements written in Markdown.

## Install

```sh
curl -fsSL https://tenet-contracts.com/install.sh | bash
export OPENROUTER_API_KEY='your-key'
```

Binaries support macOS and Linux on ARM64 and x86-64. [Provider setup](docs/installation.md) also covers TypeSafe.

## Check code

Write numbered [contracts](docs/contracts.md) under `.contracts/`, then run:

```sh
tenet validate                     # Check contract documents offline
tenet check                        # Assess the current repository
tenet check --base origin/main     # Assess changes, assuming a valid base
```

For example, a contract can require database failures to reach the caller rather than become empty results.
Tenet checks relevant files and their combined context, then reports one conclusion per contract.
Each model conclusion preserves raw confidence separately from routing. Unresolved contracts need follow-up and exit 3.
Selected-path checks assess their combined evidence and can report `clear` without claiming full contract verification.

Read every applicable contract during a review. Tenet helps choose where to investigate first; the outer reviewer owns the final judgment. See [Reviews](docs/reviews.md).

## Review changes

```sh
tenet check --base origin/main --json > review.jsonl
```

A reviewer investigates failures and unresolved contracts with ordinary repository tools. Model judgments can be wrong; a successful exit does not mean every contract was verified.
Selected code and contract rules go to the configured provider, using your API key.

Use [`tenet evidence`](docs/evidence.md) to export selected contracts and source offline for a review or a focused question.

Read [commands](docs/cli.md), [output](docs/output.md), and [reviewing with an agent](docs/reviews.md).
The same docs are at [tenet-contracts.com/docs](https://tenet-contracts.com/docs).

## Build

```sh
cargo build --release --locked -p tenet
cargo install --path crates/tenet --locked
```

[MIT](LICENSE).
