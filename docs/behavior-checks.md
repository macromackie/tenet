# Check a behavior

Write a rule in a numbered contract folder. For example, a client might accept a timeout of zero. Returning zero is only half the requirement if reading the default has a side effect.

```markdown
---
name: timeouts
message: Respect the caller's timeout.
---

If the caller supplies a timeout, use it without reading the default.
Zero is an explicit timeout. When the timeout is omitted, read the default once.
```

Save this file as `.contracts/001-timeouts/CONTRACT.md`. Set `OPENROUTER_API_KEY` in the calling environment and run `tenet check`. Tenet reviews the source, checks supported native lint settings from adopted contracts, and uses fx to find an executable example. An English rule can produce several checks if its promises can fail independently. Tenet keeps a separate conclusion for the whole contract.

Give the checker a public usage example by committing a small text file under the contract's `assets/` or `resources/` folder. Tenet includes discovered examples as references and keeps ignored and protected files out of the request.

The first generation needs Node and npm to install the pinned libfx package in the user's cache. Tenet verifies that the local sandbox tools exist before sending a generation request. Frozen checks can replay with Node and the sandbox tools offline. Generated programs run on macOS with `/usr/bin/sandbox-exec` and on Linux with bubblewrap 0.7 or later and `prlimit`. Bubblewrap must support `--size` to enforce the 64 MiB limit on temporary files. Use a recent Node with native TypeScript support for TypeScript, Python 3 for Python (Tenet adds staged package roots including `src/` layouts to `PYTHONPATH`), or `rustc` and a linker for Rust. The runner can exercise dependency-free entry points up to 128 KiB per source file (384 KiB per contract prompt) and surfaces sanitized stderr diagnostics when an isolated probe exits non-zero. An unavailable dependency, platform, or observation produces a review item. Use a [project check](./local-checks.md) for behavior that requires the project's full environment.

## Replay a finding

```sh
tenet show timeouts
tenet check --contract timeouts
```

An executable failure includes the promise and the observed result. When `OPENROUTER_API_KEY` and request budget are available, Tenet runs one bounded `fx` pass per contract to turn raw failed and unresolved findings into concise, situation-specific plain-English diagnostics without changing any recorded outcome. In interactive terminals, live progress shows each phase as it runs (`discovering source`, `generating checks`, `qualifying controls`, `repairing checks`, `replaying checks`, and `explaining findings`). Fix the implementation and rerun the same command. A review item identifies missing evidence or a concern that needs investigation. For supported lint rules, `tenet fix --contract NAME` applies safe native fixes and checks its edits.

Before adopting a generated check, Tenet exercises it against a working example, a deliberate mistake, and a second implementation with the same behavior without observing the repository's actual source during qualification. If no candidate check qualifies and request budget remains, Tenet runs one bounded repair turn using only the qualification errors. Qualified checks are materialized into concrete controls and stored in `agent-checks.lock.json` beside the contract even when another candidate in the same bundle fails qualification. Review and commit that file to share the checker with CI. It contains the program and controls. A fresh checkout qualifies the checker locally.

## Caching and cache invalidation

Generated behavior checks use two distinct cache layers so ordinary code edits stay fast and offline-replayable without hiding real regressions:

1. **Committed pin (`agent-checks.lock.json` beside `CONTRACT.md`)** stores the qualified check programs, their materialized controls (`good`, `defect`, and `refactor`), any manual-review gaps (`open`), the `contractHash`, and the public surface `shape`. Tenet automatically invalidates and regenerates checks when `CONTRACT.md` changes, when the governed public surface or manifests change, when a referenced subject file is removed, or when you run `tenet check --fresh`. You can also review, edit, or prune `agent-checks.lock.json` directly; Tenet protects replacements with atomic compare-and-swap writes so concurrent edits are never silently overwritten.
2. **Local replay receipt (`.tenet/agent-checks/`)** caches observed pass/fail/unresolved outcomes keyed by the exact governed file bytes, any scoped package data, the pin digest, the comparison base commit, and the sandbox runner version. Any byte edit to a governed source file immediately invalidates the local receipt and replays the frozen checks cleanly in the local sandbox without spending a model request.

## Mixing automated checks and manual review

One contract can combine automated behavior checks with clauses that require semantic or manual review. A generated pin separates what it can verify in isolation (`checks`) from what needs broader context, external services, or human judgment (`open`). Tenet surfaces both: executable findings report `PASS` or `FAIL`, unresolved clauses stay visible as `REVIEW`, and the semantic model review still evaluates the whole contract document.

Keep closely related clauses in one contract when they share one clear concern and file scope—for example, a pure input-handling rule paired with a cross-module invariant. Split into separate contracts when concerns or `paths` scopes diverge: Tenet asks one semantic question per contract per file, so smaller single-concern contracts produce sharper model reviews and cleaner cache invalidation.

## Review a pull request

On a supported GitHub pull-request checkout, `tenet check` selects the merge base. Fetch enough Git history for that comparison. Use `--base REF` to choose an exact base, or `--full` for the current repository. An inferred PR review establishes a new full baseline when the PR changes a governing contract. A model concern can be settled with a reason and source evidence. A currently failing behavior check remains visible after a favorable review record.

For scripts, `tenet check --json` writes JSON Lines. A `behavior_checks` event holds observations, and the closing summary carries the process exit code. With `--final --json`, the command writes one document containing `contracts`, `checks`, and `behavior_checks`. A passing example covers the input and behavior it exercised. Read unresolved rows to see which promises still lack evidence.

The `generation` object reports attempted probe-generation requests and their returned cost in US dollars (`requests`, `returned_cost_usd`), and reports bounded `fx` failure-explanation usage separately (`explanation_requests`, `explanation_cost_usd`). A missing price appears as `null`. Each finding also carries `reused` (for `.tenet/agent-checks/` local receipt cache hits) and `pin_reused` when a matching `agent-checks.lock.json` pin was replayed without generating new probes. Jev request receipts remain separate. New checker generation uses the remaining `--max-requests` allowance after semantic review. Snapshots use the reproducible semantic evaluation path. An explicit Jev endpoint or an external receipt for a final check does not send source to another router.
