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
Failures include evidence. Unresolved contracts need follow-up but do not fail the command.

## Test contracts

```sh
tenet eval --contract database-failures
tenet eval fixtures/cache --case broken-cache-hit --strict
```

[Evaluations](docs/evaluation.md) compare judgments with expected answers. Use inline examples for a small code fragment, or repository fixtures for a full directory and optional patch. Expected answers are never sent to the model.

## Review changes

```sh
tenet check --base origin/main --json > review.jsonl
```

A reviewer investigates failures and unresolved contracts with ordinary repository tools. Model judgments can be wrong; a successful exit does not mean every contract was verified.
Selected code and contract rules go to the configured provider, using your API key.

Read [commands](docs/cli.md), [output](docs/output.md), and [reviewing with an agent](docs/reviews.md).
The same docs are at [tenet-contracts.com/docs](https://tenet-contracts.com/docs).

## Build

```sh
cargo build --release --locked -p tenet
cargo install --path crates/tenet --locked
```

[MIT](LICENSE).
