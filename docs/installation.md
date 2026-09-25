# Installation

Install the latest release on macOS or Linux, on ARM64 or x86-64:

```sh
curl --proto '=https' --tlsv1.2 -LsSf https://tenet-contracts.com/install.sh | sh
tenet --version
```

For version 0.2.0 specifically, use `/v0.2.0/install.sh`.
Archives and SHA-256 checksums are also available on [GitHub Releases](https://github.com/macromackie/tenet/releases).

To build from a source checkout:

```sh
cargo install --path crates/tenet --locked
```

## Provider

```sh
export OPENROUTER_API_KEY='your-key'
tenet check src/
```

To use TypeSafe directly:

```sh
export TYPESAFE_API_KEY='your-key'
tenet check --provider typesafe
```

Tenet uses the same provider adapter as ev-grep. Supported model IDs are pinned:

| Provider | Model |
| --- | --- |
| `openrouter` | `typesafe/jev-1.13-20260917` |
| `typesafe` | `jev-1.13.0` |

`--provider` and `--model` override `TENET_PROVIDER` and `TENET_MODEL`, then the defaults.
Credentials come from the provider's environment variable. File contents are sent to that provider.
