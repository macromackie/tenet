# Limits

File judgments receive the contract plus a record containing the path, change kind, patch, and prior contents, with the resulting file supplied once.
Full checks use additions from an empty starting point. Diff checks assume the base satisfies the selected contracts.

Combined assessment receives the scoped file inventory, all captured selected source within the contract scope,
explicit context, and complete selected changes including prior contents and patches. It does not read unselected source automatically.
Selected subjects and support files are labeled separately; support files do not expand the requested claim.
It can detect missing required files and inspect cross-file requirements when the context fits. Relevance answers
do not prune selected evidence. Conflicting file failures remain visible after the repository assessment.
It does not retrieve external dependencies, execute the commands written in contracts, or run an agent investigation.

Each source file and encoded change or repository context for model assessment is limited to 64 KiB. Queries are limited to 8 KiB, encoded provider requests to 96 KiB, and responses to 64 KiB.
Oversized selected inputs produce errors. Repository context that cannot be supplied completely leaves the contract unresolved. Inputs are never silently truncated.
Large scopes may therefore need a reviewing agent to complete the assessment. Narrowing positional paths or explicit
context changes the supplied evidence; record the remaining scope instead of treating a smaller check as full coverage.

Checks, dry runs, and exports also have a fixed 64 MiB aggregate capture limit. It counts serialized contract data
and original documents, plus distinct source records including paths, patches, and read errors. This bounds the
captured payload, not total process memory. Capture overflow exits 2 before any model requests. Increasing the
request budget does not raise this bound.

The offline [`evidence` export](./evidence.md#bounds-and-errors) has a separate packet limit; it does not enlarge model inputs.

The shared request budget covers applicability, verification, and repository assessments, including failed attempts.
Each file uses one request that asks relevance and violation together. A contract may use one additional repository request.
File and repository assessments share the jobs limit across independent contracts. Final summaries retain scope and numeric order.

Jev requests share a 15-second budget across at most three attempts, with a five-second limit per attempt. One slow request may use a spare worker to race a duplicate after one second. Rate limits pause new requests; the first valid answer wins. Duplicate or interrupted requests may still incur charges.
Model answers can differ between runs on the same input, especially for borderline files. Compare repeated runs
before treating a change in results as an improvement.

Every answer uses `--min-confidence` (default 0.8). Lower-confidence and explicit uncertain answers route to
uncertain while their raw choices and probabilities remain visible. Every relevance outcome retains selected source
in combined evidence; uncertain file judgments may be resolved by a confident combined assessment. The 0.8 floor is a routing
policy, not a measured accuracy guarantee. Lowering it changes routing and does not improve the evidence.
