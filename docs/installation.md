# Installation

Install the latest release on macOS or Linux, on ARM64 or x86-64:

```sh
curl -fsSL https://tenet-contracts.com/install.sh | bash
tenet --version
```

To pin a release, use its path, such as `/v0.33.0/install.sh`.
Archives and SHA-256 checksums are also available on [GitHub Releases](https://github.com/macromackie/tenet/releases).

To build from a source checkout:

```sh
cargo install --path crates/tenet --locked
```

## Provider

```sh
export OPENROUTER_API_KEY='your-key'
tenet doctor
tenet init
tenet check
```

To use TypeSafe directly:

```sh
export TYPESAFE_API_KEY='your-key'
tenet check --provider typesafe
```

For automatic behavior checks, set `OPENROUTER_API_KEY` and use the default OpenRouter endpoint. Tenet sends the contract, selected project source and reference examples to its fx generation model, `openai/gpt-6-luna` by default. Override it with `TENET_AGENT_MODEL`. See [behavior checks](./behavior-checks.md) for local runtime requirements.

Semantic review uses the same provider adapter as ev-grep. Supported model IDs are pinned:

| Provider     | Default model                | Other supported model        |
| ------------ | ---------------------------- | ---------------------------- |
| `openrouter` | `typesafe/jev-1.14-20261014` | `typesafe/jev-1.13-20260917` |
| `typesafe`   | `jev-1.14.0`                 | `jev-1.13.0`                 |

`--provider` and `--model` override `TENET_PROVIDER` and `TENET_MODEL`, then the defaults.
Credentials come from the provider's environment variable. File contents are sent to that provider.

Select a pinned model explicitly when a review or comparison needs it:

```sh
tenet check --model typesafe/jev-1.14-20261014
```

## Provider proxy

Use `--endpoint URL` or `TENET_ENDPOINT` for a trusted proxy that implements the selected provider protocol. Requests and the provider credential go to that URL. Use HTTPS for remote services; HTTP is supported for local development. Redirects are disabled.
