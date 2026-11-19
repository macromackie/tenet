# Tenet

Check code against requirements written in Markdown.

## Install

```sh
curl -fsSL https://tenet-contracts.com/install.sh | bash
```

Binaries support macOS and Linux on ARM64 and x86-64. Local checks run with the tools installed in your project. For model review, set `OPENROUTER_API_KEY` or configure TypeSafe. See [Provider setup](docs/installation.md).

## Check code

Write a numbered [contract](docs/contracts.md) under `.contracts/`. For example, “When the caller supplies a timeout, including zero, use it without reading the default.” Then run:

```sh
tenet check                   # Review the repo or the current GitHub pull request
tenet show timeouts           # Inspect a finding and its evidence
tenet fix                     # Apply supported safe native lint fixes
tenet check --contract timeouts
```

With OpenRouter configured, a supported Linux checkout can build a behavior check, exercise it against a deliberate defect, and save it for the next run. See [behavior checks and runner requirements](docs/behavior-checks.md). Use [local checks](docs/local-checks.md) for project-owned tests and linters, and [final checks](docs/final-checks.md) to verify the finished change.

For example, a contract can require database failures to reach the caller rather than become empty results.
Tenet checks relevant files and their combined context, then reports one conclusion per contract.
Each model conclusion preserves raw confidence separately from routing. A model's failed or unresolved conclusion stays pending, with exit 4, until a reviewer records one.
Selected-path checks assess their combined evidence and can report `clear` without claiming full contract verification.

Install shared contracts from the public [tenet-contracts](https://github.com/macromackie/tenet-contracts) library:

```sh
tenet contracts recommend           # See suitable contracts before installing them
tenet contracts add tenet/data-and-apis --scope packages/api
tenet contracts update             # Take releases up to the next major, keeping local edits
```

See [Guidance](docs/guidance.md) for local adaptations and conflicts.

Read every applicable contract during a review. Tenet helps choose where to investigate first; the outer reviewer owns the final judgment. See [Reviews](docs/reviews.md).

## Review changes

```sh
tenet check --base origin/main     # Assess the change and append a revision to .tenet/
tenet status                       # Settled and pending contracts, without a model
tenet show database-failures       # The model's answer, history, and the record command
tenet record database-failures --conclusion failed --evidence src/query.ts:40-52 -m "returns [] on failure"
```

A reviewer investigates pending contracts with ordinary repository tools and records a conclusion for each. A record
is bound to the exact code it judged; editing a scoped file makes it outdated. Model judgments can be wrong; a
successful exit does not mean every contract was verified.
Selected code and contract rules go to the configured provider, using your API key.

Use [`tenet export`](docs/export.md) to export selected contracts and source offline for a review or a focused question.

For a larger review, `check --contract-batch-size 4` can ask about several contracts together while supplying a
file's source once per request. The default remains one contract per request. `--context-profile long` raises the
input byte limits for a check or export; it does not establish that a model can admit or accurately review that input.
Preview the selected work with `check --dry-run --json`. See [request options](docs/cli.md#assessment-inputs) and
[limits](docs/limits.md).

Add requirements one at a time and run a full check of each new contract. Repeated checks reuse unchanged file/contract
answers; a changed file or new contract adds work without requiring every pair to be asked again. Combined assessments
also depend on the other files in their scope. See [incremental adoption](docs/reviews.md#adopt-requirements-incrementally).

Read [commands](docs/cli.md), [output](docs/output.md), and [reviewing with an agent](docs/reviews.md).
The same docs are at [tenet-contracts.com/docs](https://tenet-contracts.com/docs).

## Build

```sh
cargo build --release --locked -p tenet
cargo install --path crates/tenet --locked
```

The Rust binary embeds the checker worker. To change it, use Node 22.23.2 and regenerate the bundle before building.

```sh
cd checker
npm ci --ignore-scripts
npm run typecheck
npm test
npm run bundle
```

CI rebuilds the worker and checks it against the committed copy. A fresh checkout can replay a frozen check with Node and the sandbox tools for its language. The first model-generated check also installs the pinned fx library in the user cache.

Package ownership and allowed crate dependencies are declared in [package-boundaries.json](package-boundaries.json).
CI verifies that declaration against Cargo and source imports, alongside the source-copy and advisory checks.

The package check requires Cargo, Python 3.14 or later, and uv. From the source checkout, verify the released checker
and run it against the Cargo policy:

```sh
shasum -a 256 --check .checks/package-boundaries/SHA256SUMS
uv run --locked --script .checks/package-boundaries/check-package-boundaries.py --root . --config package-boundaries.json
```

Policies that include JavaScript or TypeScript packages also install the npm resolver described in the bundled guide.

[MIT](LICENSE).
