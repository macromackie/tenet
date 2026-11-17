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

`check` and `export` use the `standard` context profile unless you pass `--context-profile long`.
The profile sets byte bounds for each encoded file input, combined assessment, and all explicit context together:

| Profile    | Tenet evidence | Provider state | Encoded provider request |
| ---------- | -------------- | -------------- | ------------------------ |
| `standard` | 64 KiB         | 96 KiB         | 96 KiB                   |
| `long`     | 1 MiB          | 1 MiB          | 2 MiB                    |

Queries remain limited to 8 KiB each and responses to 64 KiB. A profile is a byte admission bound, not a token count or
a claim about model accuracy. The provider's token window and request policy can reject an input within these bounds;
Tenet retains that failure and leaves the affected scope unresolved while other admissible checks continue.
Tenet does not automatically select the long
profile, sample source, or raise a limit when an input is too large.

All `--context` files together count toward the selected evidence bound as the model receives them: each file's resulting contents, or
the numbered lines of an [excerpt](./reviews.md#add-context). Prior contents and patches of context files are not sent,
so they do not count. Oversized context leaves affected file checks incomplete and their combined scope unresolved;
Tenet sends no oversized request. Supply focused line ranges, select the long profile, or inspect the complete evidence
and [record a conclusion](./reviews.md). An export can retain that context within its own packet bound.
A file whose input exceeds the limit with only its patch left is `incomplete`.
For diffs, Tenet automatically checks consecutive complete hunks in groups that fit the same limit.
Each group includes the contract, explicit context, and its old and new line ranges.
An indivisible hunk that still exceeds the limit stays unexamined with its ranges and byte count reported.
Large additions and deletions can contain one indivisible hunk.
The file stays incomplete even when every group answers favorably, because a rule may depend on relationships across groups.
`show` retains the hunk answers and the combined-review outcome for a reviewer to settle.
Full checks require complete files and remain incomplete when their inputs exceed the limit.
Empty, binary, non-UTF-8, and symbolic-link files are `skipped`: they stay in the inventory, and no model judges their contents.
If the reduced repository context still exceeds the limit, the file checks run anyway. A file check that finds a violation fails the contract with `file_violation`; otherwise the contract stays unresolved with `context_too_large` and its byte counts. Inputs are never silently truncated.
Large scopes may therefore need a reviewing agent to complete the assessment. Narrowing positional paths or explicit
context changes the supplied evidence; record the remaining scope instead of treating a smaller check as full coverage.

Checks and dry runs retain at most 64 MiB of source and 64 MiB of encoded capture data, including contract documents,
paths, patches, excerpts, and read errors. A source file or aggregate payload that exceeds these bounds stays in scope
as incomplete evidence. Tenet defers complete larger payloads so other admissible checks can continue; it never samples
or silently truncates a file. A combined assessment that needs deferred source remains unresolved. Increasing the request
budget does not raise these bounds.

Tenet streams readable deferred files to retain their exact content hashes. A same-size edit still invalidates a recorded
conclusion. A file that cannot be read stably has no complete identity: substantive approvals and failures cannot stay current,
and only an unresolved conclusion can be recorded until the source is available. Large readable files can still support
manual records; citation reads retain at most the review's 1 MiB blob bound. A favorable diff record also needs captured
changed-line evidence, or a whole deleted file; inspect a full check when the diff cannot supply that evidence.

The capture bounds apply to retained data, not total process memory or hashing work. Contract documents and source metadata
must themselves fit the fixed capture bound. If documents or path and error records alone exhaust it, preparation exits 2
before model requests. This resource refusal is separate from an oversized source file's incomplete assessment.
Exports have the same bounded source capture and refuse a packet whose complete source cannot be retained.

The offline [`export` command](./export.md#bounds-and-errors) has a separate packet limit; it does not enlarge model inputs.

`tenet contracts update` merges line by line with `git merge-file`. Rewrapping a paragraph changes every line in it,
so an upstream edit to that paragraph conflicts as a whole. Shared contracts keep one sentence per line to limit this.
The merge does not understand Markdown or meaning; review every merged contract.

The shared request budget counts logical model requests for file and repository assessments, including requests that fail.
By default, each ordinary file check uses one request that asks relevance and violation together.
With `--contract-batch-size N`, several contracts for the same file can share a request. Each contract still receives
its own relevance and violation answers, and the encoded input and question limits can split a group below N.
The default is 1; grouping is opt-in. It changes request construction and has no implied speed or quality guarantee.
For an oversized diff, each admissible hunk group uses one such request, with completed answers cached for reruns.
A contract may use one additional repository request.
Answers reused from earlier checks spend none of it. `check --dry-run` predicts the requests to send and warns when
they exceed `--max-requests`; a check that reaches the budget leaves
the rest `incomplete` and its summary says how many requests it did not send. Measured on October 15, 2026, a request
to OpenRouter's default model cost about $0.0002 to $0.0003, so a pull request needing about 100 requests costs about $0.03.
File and repository assessments share the jobs limit across independent contracts. Final summaries retain scope and numeric order.
The request cap is not a spending cap: one logical request can make several provider attempts, and a raced or interrupted
attempt may still incur a charge. Available token and attempt counts describe observed usage; missing usage is not zero cost.

Jev requests share a 15-second budget across at most three attempts, with a five-second limit per attempt. One slow request may use a spare worker to race a duplicate after one second. Rate limits pause new requests; the first valid answer wins. Duplicate or interrupted requests may still incur charges.
A request that still fails leaves its contract pending; a rerun sends it again. See [failed requests](./output.md#failed-requests).
Model answers can differ between runs on the same input, especially for borderline files. A repeated check reuses the
earlier answers, so compare `--fresh` runs before treating a change in results as an improvement.

Every answer uses `--min-confidence` (default 0.8). Lower-confidence and explicit uncertain answers route to
uncertain while their raw choices and probabilities remain visible. Every relevance outcome retains selected source
in combined evidence; uncertain file judgments may be resolved by a confident combined assessment. The 0.8 floor is a routing
policy, not a measured accuracy guarantee. Lowering it changes routing and does not improve the evidence.
