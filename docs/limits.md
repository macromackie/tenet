# Limits

File judgments receive a contract's rules and applicability guidance plus a record containing the path, change kind, and before/after contents.
Full checks use additions from an empty starting point. Diff checks assume the base satisfies the selected contracts.

Contract assessment receives the scoped file inventory, current source contents, change records, and file judgments. It can detect missing required files and inspect cross-file requirements when the context fits.
It does not retrieve external dependencies, execute the commands written in contracts, or run an agent investigation.

Each source file and encoded change or repository context is limited to 64 KiB. Queries are limited to 8 KiB, encoded provider requests to 96 KiB, and responses to 64 KiB.
Oversized selected inputs produce errors. Repository context that cannot be supplied completely leaves the contract unresolved. Inputs are never silently truncated.
Large scopes may therefore need a reviewing agent to complete the assessment.

The shared request budget covers applicability, verification, and repository assessments, including failed attempts.
A file uses one request when excluded and up to two otherwise. A contract may use one additional repository request.
File requests run concurrently within the jobs limit; contracts finish sequentially in scope and numeric order.

The provider adapter uses a 10-second connection timeout and a 60-second request timeout, with no automatic retries.
Confidence below 0.8 maps to uncertainty. This provisional threshold is not a measured guarantee of accuracy.
