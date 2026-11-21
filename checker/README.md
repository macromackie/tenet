# Tenet Checker Worker

Embedded Node.js worker (`worker.bundle`) invoked by `crates/tenet-checks` to generate, qualify, and replay executable behavior checks inside an OS sandbox.

## Dataflow

```text
Rust tenet-checks (env_clear + real node)
  -> worker.mts          stdin/stdout JSON protocol (wire.mts)
  -> discovery/          bounded PROMPT (300 KiB) & EXECUTION (16 MiB) inventories, path ranking
  -> review/select-pin   reuse matching agent-checks.lock.json or generate + qualify via fx/
  -> sandbox/            stage controls & run probes inside macOS Seatbelt or Linux bwrap/prlimit
  -> review/receipt/     write atomic replay receipts keyed by file bytes, pin digest & runner version
  -> fx/explain-findings rewrite failed/unresolved diagnostics read-only (never alters outcomes)
```

## Subpackages

| Folder              | Responsibility                                                                                   |
| ------------------- | ------------------------------------------------------------------------------------------------ |
| `discovery/`        | File selection, size/count budgets, manifest inclusion, and path ranking (`rank-path.mts`).      |
| `fx/`               | OpenRouter transport, `libfx` agent turns, candidate generation, and read-only explanations.     |
| `review/pin/`       | `agent-checks.lock.json` schema, matching, bundle qualification, and pin updates.                |
| `review/receipt/`   | Evidence digests, local `.tenet/agent-checks/` receipt caching, and atomic writes.               |
| `sandbox/runtimes/` | Toolchain probes (`node`, `python`, `rust`, `go`), Cargo manifest parsing, and command builders. |
| `sandbox/os/`       | macOS Seatbelt (`sandbox-exec`), Linux `bubblewrap`/`prlimit`, and preflight checks.             |
| `sandbox/controls/` | Good/defect/refactor control application, check validation, and failure diagnostics.             |
| `storage/`          | Bounded regular-file reads, path safety, and atomic writes.                                      |

## Adding a Runtime

1. Add toolchain probe and command/env builders under `sandbox/runtimes/` and export from `sandbox/runtimes/index.mts`.
2. Wire the runtime into `wire.mts`, `discovery/discovery.mts`, `discovery/manifests.mts`, `sandbox/project-deps.mts`, `sandbox/runner-sandbox.mts`, `sandbox/runner-version.mts`, `sandbox/os/{macos,linux}-sandbox.mts`, and `checker-prompt.md`.
3. Update `crates/tenet-checks/src/behavior/environment.rs` (trusted tool names + env vars) and `crates/tenet/src/commands/doctor/environment.rs`.
4. Add unit + real-sandbox isolation tests under `test/`, rebuild `worker.bundle` (`npm run bundle`), and list new files in `.agents/skills/publish-tenet/resources/export.json`.

## Verification

```sh
npm --prefix workspaces/core/checker run typecheck
npm --prefix workspaces/core/checker run check:bundle
npm --prefix workspaces/core/checker test
```
