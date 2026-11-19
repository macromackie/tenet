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

The first generation needs Node and npm to install the pinned libfx package in the user's cache. Frozen checks can replay with Node and the sandbox tools offline. Generated programs run on Linux with bubblewrap 0.7 or later, prlimit and the language runtime. Bubblewrap must support `--size` to enforce the 64 MiB limit on temporary files. Use a recent Node with native TypeScript support for TypeScript, Python 3 for Python, or rustc and a linker for Rust. The runner can exercise small dependency-free entry points. An unavailable dependency, platform, or observation produces a review item. Use a [project check](./local-checks.md) for behavior that requires the project's full environment.

## Replay a finding

```sh
tenet show timeouts
tenet check --contract timeouts
```

An executable failure includes the promise and the observed result. Fix the implementation and rerun the same command. A review item identifies missing evidence or a concern that needs investigation. For supported lint rules, `tenet fix --contract NAME` applies safe native fixes and checks its edits.

Before adopting a generated check, Tenet exercises it against a working example, a deliberate mistake, and a second implementation with the same behavior. A qualified check is stored in `agent-checks.lock.json` beside the contract. Review and commit that file to share the checker with CI. It contains the program and controls. A fresh checkout qualifies the checker locally. Unchanged results are reusable in the checkout's review state. A changed requirement or public surface can require a new checker.

## Review a pull request

On a supported GitHub pull-request checkout, `tenet check` selects the merge base. Fetch enough Git history for that comparison. Use `--base REF` to choose an exact base, or `--full` for the current repository. An inferred PR review establishes a new full baseline when the PR changes a governing contract. A model concern can be settled with a reason and source evidence. A currently failing behavior check remains visible after a favorable review record.

For scripts, `tenet check --json` writes JSON Lines. A `behavior_checks` event holds observations, and the closing summary carries the process exit code. With `--final --json`, the command writes one document containing `contracts`, `checks`, and `behavior_checks`. A passing example covers the input and behavior it exercised. Read unresolved rows to see which promises still lack evidence.

The `generation` object reports attempted checker requests and their returned cost in US dollars. A missing price appears as `null`. Jev request receipts remain separate. New checker generation uses the remaining `--max-requests` allowance after semantic review. Snapshots use the reproducible semantic evaluation path. An explicit Jev endpoint or an external receipt for a final check does not send source to another router.
