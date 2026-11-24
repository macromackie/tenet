# Behavior checks

Write a rule in a numbered contract folder. For example, a client might accept a timeout of zero. Returning zero is only half the requirement if reading the default has a side effect.

```markdown
---
name: timeouts
message: Respect the caller's timeout.
---

If the caller supplies a timeout, use it without reading the default.
Zero is an explicit timeout. When the timeout is omitted, read the default once.
```

Save this file as `.contracts/001-timeouts/CONTRACT.md`. Set `OPENROUTER_API_KEY` in the calling environment and run `tenet check` or `tenet scan --apply`. Tenet reviews the source, runs supported native lint settings from adopted contracts, and uses `fx` to build qualified executable checks. One English rule can produce multiple checks if its promises can fail independently. Tenet keeps a separate conclusion for the whole contract.

Commit a public usage example under the contract's `assets/` or `resources/` folder to give the generator concrete entry points. Tenet includes discovered examples as references and keeps ignored and protected files out of model requests.

## Materialized scripts and sandbox execution

Tenet writes qualified checks into the contract folder as standalone runnable scripts beside a lock manifest:

```text
.contracts/
  001-timeouts/
    CONTRACT.md
    scripts/
      explicit-zero.mjs
      default-read-once.mjs
      .lock.json
```

Each script at `scripts/<check>.<ext>` runs directly in the OS sandbox, and `scripts/.lock.json` records its SHA-256 digest, a compact SHA-256 `controlHash` covering its qualification controls (`good`, `defect`, and `refactor`), any manual-review gaps (`open`), `contractHash`, and the governed public surface `shape` (legacy lock manifests with embedded control sources remain readable).

Before adopting a generated script, Tenet qualifies it against a working control, a deliberate defect, and an equivalent refactor without observing the repository's actual source during qualification. If no candidate qualifies and request budget remains, Tenet runs one bounded repair turn using only the qualification errors. Qualified checks write to `scripts/` and `scripts/.lock.json` even when another candidate in the same bundle fails qualification. Review and commit `scripts/` to share checks across developers and CI.

The first generation requires Node and npm to install the pinned `libfx` package in the user cache. Tenet verifies local sandbox tools before sending a generation request:

- **macOS:** `/usr/bin/sandbox-exec`.
- **Linux:** `bubblewrap` 0.7 or later (with `--size` support to enforce the 64 MiB temporary-file limit) and `prlimit`.
- **Language runtimes:** Node with native TypeScript support for JavaScript and TypeScript, Python 3 for Python (Tenet adds staged package roots including `src/` layouts to `PYTHONPATH`), `rustc` and a linker for Rust, or the `go` toolchain for `.go` scripts.

When in-root project dependencies already exist on disk (`node_modules`, `.venv` or `venv` `site-packages`, or prebuilt Rust artifacts in `target/debug/deps` from `cargo build`), Tenet canonicalizes and mounts them read-only inside the sandbox without running installs or Cargo build scripts. Discovery prioritizes manifests, flagged files, and contract-referenced modules ahead of generated (`gen/`, `generated/`), compatibility (`_future/`, `compat/`), and vendor paths up to 128 KiB per file (384 KiB per contract prompt). Isolated probes that exit non-zero surface sanitized diagnostics (`ERR_MODULE_NOT_FOUND`, `ModuleNotFoundError`, `E0432`/`E0463`). An unavailable dependency, platform, or observation produces a review item. Use a [project check](./local-checks.md) for behavior that requires the project's full environment.

## Replay offline or after a fix

Replay locked runtime scripts without calling an LLM or setting provider credentials:

```sh
tenet check --offline
tenet check --offline --contract timeouts
tenet show timeouts
```

`tenet check --offline` (alias `--replay`) executes static lint checks, declared local checks, and locked scripts in `scripts/` (including ephemeral contracts under `.tenet/scan/.contracts/`) inside the local sandbox, then persists a receipt at `.tenet/offline-check.json` so `tenet status` reports those results. `tenet show timeouts` prints the contract rule, observed evidence, diff, and the direct runnable path to each script under `.contracts/001-timeouts/scripts/`.

When `OPENROUTER_API_KEY` and request budget are available during `tenet check`, Tenet runs one bounded `fx` pass per contract to turn raw failed and unresolved findings into situation-specific plain-English diagnostics without altering recorded outcomes. Interactive terminals stream live progress phases (`discovering source`, `generating checks`, `qualifying controls`, `repairing checks`, `replaying checks`, and `explaining findings`).

Run `tenet fix` to apply safe native lint fixes and repair failing runtime scripts in a bounded sandboxed verify-until-green loop. Pass `--contract timeouts` to scope repairs to one contract or `--severity` to filter by severity. After editing the code manually, rerun `tenet check --offline --contract timeouts` or `tenet check --contract timeouts`.

## Hand-edit protection and caching

Generated behavior checks use two cache layers and a hash guard so local edits stay safe and code changes trigger immediate replay:

1. **Committed scripts and lock (`scripts/<check>.<ext>` and `scripts/.lock.json`):** Stores the runnable scripts, their SHA-256 digests, compact SHA-256 `controlHash`, open clauses, `contractHash`, and surface `shape` (legacy lock entries with embedded control sources remain readable). Tenet invalidates and regenerates unedited checks when `CONTRACT.md` changes, when the governed public surface or manifests change, when a referenced subject file is removed, or when you run `tenet check --fresh`. If you edit a script file by hand, its SHA-256 digest no longer matches `scripts/.lock.json`; Tenet detects the hand edit, preserves your script, and refuses to overwrite it silently. Lock updates use atomic compare-and-swap writes so concurrent edits are never lost.
2. **Local replay receipt (`.tenet/agent-checks/`):** Caches pass, fail, and unresolved observations keyed by exact governed file bytes, scoped package data, the lock digest, the comparison base commit, and the sandbox runner version. Any byte edit to a governed source file invalidates the local receipt and replays the locked scripts in the sandbox without generating new probes.

### Legacy pin compatibility

Repositories with existing committed `agent-checks.lock.json` files beside `CONTRACT.md` continue to replay during both `tenet check` and `tenet check --offline`. Tenet reads `scripts/.lock.json` first and falls back to `agent-checks.lock.json` when no `scripts/` manifest exists. Newly generated or refreshed checks materialize into `scripts/` and `scripts/.lock.json`.

## Mixing automated checks and manual review

One contract can pair automated runtime scripts with clauses that require semantic or manual review. A lock manifest separates isolated executable checks (`checks`) from clauses that need broader context, external services, or human judgment (`open`). Tenet surfaces both: executable scripts report `PASS` or `FAIL`, unresolved clauses stay visible as `REVIEW`, and semantic model review still evaluates the whole contract document.

Keep closely related clauses in one contract when they share one concern and file scope, such as an input-handling rule paired with a cross-module invariant. Split clauses into separate contracts when concerns or `paths` scopes diverge, because Tenet asks one semantic question per contract per file.

## Pull requests and JSON output

On a supported GitHub pull-request checkout, `tenet check` selects the merge base. Fetch enough Git history for that comparison. Use `--base REF` to choose an exact base, `--full` to check the entire repository, or `--offline` to replay locked scripts in CI without model credentials. An inferred PR review establishes a new full baseline when the PR edits a governing contract. A currently failing behavior check remains visible even after a favorable review record.

For scripts, `tenet check --json` and `tenet check --offline --json` write JSON Lines. A `behavior_checks` event holds observations (including optional `severity` on findings), and the closing summary carries the process exit code. With `--final --json`, the command writes one document containing `contracts`, `checks`, and `behavior_checks`. A passing example covers only the input and behavior it exercised; read unresolved rows to see which promises still lack evidence.

The `generation` object reports attempted probe-generation requests and returned cost in US dollars (`requests`, `returned_cost_usd`), and reports bounded `fx` failure-explanation usage separately (`explanation_requests`, `explanation_cost_usd`). A missing price appears as `null`. Each finding also carries `reused` (for `.tenet/agent-checks/` receipt cache hits) and `pin_reused` when a matching `scripts/.lock.json` or legacy `agent-checks.lock.json` pin replayed without generating new probes. Jev request receipts remain separate. New script generation uses the remaining `--max-requests` allowance after semantic review. Snapshots use the reproducible semantic evaluation path. An explicit Jev endpoint or external receipt for a final check does not send source to another router.
