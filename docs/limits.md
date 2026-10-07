# Limits

File judgments receive the contract plus a record containing the path, change kind, patch, and prior contents, with the resulting file supplied once.
Full checks use additions from an empty starting point. Diff checks assume the base satisfies the selected contracts.

Contract assessment receives the scoped file inventory, relevant current source, explicit context, and changed-file
paths and kinds. Prior contents and patches are supplied to file checks, not the final repository assessment.
It can detect missing required files and inspect cross-file requirements when the context fits. File judgments determine relevance; conflicting file failures
remain visible after the repository assessment.
It does not retrieve external dependencies, execute the commands written in contracts, or run an agent investigation.

Each source file and encoded change or repository context is limited to 64 KiB. Queries are limited to 8 KiB, encoded provider requests to 96 KiB, and responses to 64 KiB.
Oversized selected inputs produce errors. Repository context that cannot be supplied completely leaves the contract unresolved. Inputs are never silently truncated.
Large scopes may therefore need a reviewing agent to complete the assessment.

The shared request budget covers applicability, verification, and repository assessments, including failed attempts.
Each file uses one request that asks relevance and violation together. A contract may use one additional repository request.
File and repository assessments share the jobs limit across independent contracts. Final summaries retain scope and numeric order.

Jev requests share a 15-second budget across at most three attempts, with a five-second limit per attempt. One slow request may use a spare worker to race a duplicate after one second. Rate limits pause new requests; the first valid answer wins. Duplicate or interrupted requests may still incur charges.
Model answers can differ between runs on the same input, especially for borderline files. Compare repeated runs
before treating a change in results as an improvement.

A relevance answer below 0.8 confidence counts as uncertain, so the file is still judged. Violation answers keep
their choice and report confidence without a threshold. The 0.8 floor is provisional, not a measured accuracy guarantee.
