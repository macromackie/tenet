# Tenet

Check code against Markdown contracts.

```sh
tenet validate
tenet eval
tenet check --changed-since origin/main
```

A contract contains a requirement and examples of passing and failing code. `eval` tests those examples through the same relevance and verification passes used by `check`.

Tenet uses Jev through OpenRouter or TypeSafe. Set `OPENROUTER_API_KEY`, or use `--provider typesafe` with `TYPESAFE_API_KEY`.

```sh
tenet contracts list
tenet contracts view database-failures
tenet check src/ --jobs 4
tenet check --reporter jsonl
```

A reviewing agent investigates failures and unresolved checks using its normal tools. Tenet does not manage a persistent review.
Changed-file checks evaluate current contents; findings may predate the diff.

Read [contracts](docs/contracts.md), [evaluation](docs/evaluation.md), [commands](docs/cli.md), and [output](docs/output.md).

## Install

```sh
curl --proto '=https' --tlsv1.2 -LsSf https://tenet-contracts.com/install.sh | sh
```

Prebuilt binaries support macOS and Linux on ARM64 and x86-64.
See [installation](docs/installation.md) for provider setup and versioned releases.

## Build

```sh
cargo build --release --locked -p tenet
cargo install --path crates/tenet --locked
```

## License

[MIT](LICENSE).
