# Limits

File judgments receive the contract plus a record containing the path, change kind, patch, and prior contents, with the resulting file supplied once.
When that input exceeds the limit, Tenet leaves out the prior contents and then the resulting contents, but only while the patch still shows the change.
Lines of the file named with [`--context PATH:START-END`](./reviews.md#add-context) take the place of left-out resulting contents when they fit.
The result lists each omission in `omitted`, and the model is told to answer uncertain when it needs the missing text.
Full checks use additions from an empty starting point. Diff checks assume the base satisfies the selected contracts
and judge only what the change introduces, extends, or worsens.

Combined assessment receives the inventory of files the contract governs, all captured selected source within the contract scope,
explicit context, and complete selected changes including prior contents and patches. It does not read unselected source automatically.
When that input exceeds the limit, Tenet leaves out prior contents, then summarizes omitted inventory paths as file counts per directory, then leaves out the resulting contents of the largest changed files.
Text is left out only where a patch still shows the change, so a full check keeps every selected file whole. Named lines of a file take the place of its left-out contents here too, and are left out last. The conclusion's `context` lists each reduction.
Selected subjects and support files are labeled separately; support files do not expand the requested claim.
It can detect missing required files and inspect cross-file requirements when the context fits. Relevance answers
do not prune selected evidence. Conflicting file failures remain visible after the repository assessment.
It does not retrieve external dependencies, execute the commands written in contracts, or run an agent investigation.

Tenet uses directory trees for inventory metadata when they encode to fewer bytes than expanded path lists.
The trees preserve every inventory path and distinguish supplied source from paths whose contents are omitted.
Selected subjects and supporting files remain separate, and tree nesting is bounded.
When even the tree does not fit, omitted paths become file counts per directory, from the deepest directories up to one total for the scope.
Offline evidence packets list paths without trees.
A contract's [`paths`](./contracts.md#scope) patterns shrink both its inventory and its selected source.

Each encoded file input and repository context for model assessment is limited to 64 KiB. Queries are limited to 8 KiB, encoded provider requests to 96 KiB, and responses to 64 KiB.
All `--context` files together must also fit in 64 KiB as the model receives them: each file's resulting contents, or
the numbered lines of an [excerpt](./reviews.md#add-context). Prior contents and patches of context files are not sent,
so they do not count. A check or export whose context exceeds this exits 2 before any request.
A file whose input exceeds the limit with only its patch left is `incomplete`. Empty, binary, non-UTF-8, and symbolic-link files are `skipped`: they stay in the inventory, and no model judges their contents.
If the reduced repository context still exceeds the limit, the file checks run anyway. A file check that finds a violation fails the contract with `file_violation`; otherwise the contract stays unresolved with `context_too_large` and its byte counts. Inputs are never silently truncated.
Large scopes may therefore need a reviewing agent to complete the assessment. Narrowing positional paths or explicit
context changes the supplied evidence; record the remaining scope instead of treating a smaller check as full coverage.

Checks, dry runs, and exports also have a fixed 64 MiB aggregate capture limit. It counts serialized contract data
and original documents, plus distinct source records including paths, patches, and read errors. This bounds the
captured payload, not total process memory. Capture overflow exits 2 before any model requests. Increasing the
request budget does not raise this bound.

The offline [`export` command](./export.md#bounds-and-errors) has a separate packet limit; it does not enlarge model inputs.

`tenet contracts update` merges line by line with `git merge-file`. Rewrapping a paragraph changes every line in it,
so an upstream edit to that paragraph conflicts as a whole. Shared contracts keep one sentence per line to limit this.
The merge does not understand Markdown or meaning; review every merged contract.

The shared request budget covers applicability, verification, and repository assessments, including failed attempts.
Each file uses one request that asks relevance and violation together. A contract may use one additional repository request.
Answers reused from earlier checks spend none of it. `check --dry-run` predicts the requests to send and warns when
they exceed `--max-requests`; a check that reaches the budget leaves
the rest `incomplete` and its summary says how many requests it did not send. Measured on October 15, 2026, a request
to OpenRouter's default model cost about $0.0002 to $0.0003, so a pull request needing about 100 requests costs about $0.03.
File and repository assessments share the jobs limit across independent contracts. Final summaries retain scope and numeric order.

Jev requests share a 15-second budget across at most three attempts, with a five-second limit per attempt. One slow request may use a spare worker to race a duplicate after one second. Rate limits pause new requests; the first valid answer wins. Duplicate or interrupted requests may still incur charges.
A request that still fails leaves its contract pending; a rerun sends it again. See [failed requests](./output.md#failed-requests).
Model answers can differ between runs on the same input, especially for borderline files. A repeated check reuses the
earlier answers, so compare `--fresh` runs before treating a change in results as an improvement.

Every answer uses `--min-confidence` (default 0.8). Lower-confidence and explicit uncertain answers route to
uncertain while their raw choices and probabilities remain visible. Every relevance outcome retains selected source
in combined evidence; uncertain file judgments may be resolved by a confident combined assessment. The 0.8 floor is a routing
policy, not a measured accuracy guarantee. Lowering it changes routing and does not improve the evidence.
