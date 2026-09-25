# Limits

Each judgment receives one complete file and one contract's rules and applicability guidance.
Tenet does not retrieve imports, inspect a call graph, or prove a repository-wide architectural claim.
Use uncertainty to identify checks that need more context.

Source files are limited to 64 KiB, queries to 8 KiB, encoded requests to 96 KiB, and responses to 64 KiB.
Inputs are never silently truncated. Oversized selected text files produce errors.
The provider adapter uses a 10-second connection timeout and a 60-second request timeout, with no automatic retries.

The request limit counts both applicability and verification, including attempted requests that fail.
A contract/file pair uses one request when excluded and up to two otherwise.
Exhausting the budget leaves remaining checks incomplete. Concurrency bounds requests, not billing.

The adapter currently maps confidence below 0.8 to uncertainty. This threshold is provisional, not a measured guarantee of accuracy.
A passing judgment is model evidence about the supplied file, not proof of correctness.
