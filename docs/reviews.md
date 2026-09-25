# Reviews

A reviewing agent runs Tenet, reads the report, and investigates findings with its ordinary tools.

```sh
tenet check --changed-since origin/main --reporter jsonl
```

Capture stdout and the exit status even when the command returns nonzero.
Each result identifies the assessed source and contract by content hash.
If the checkout changes, rerun affected checks before relying on earlier assessments.

| Result | Review action |
| --- | --- |
| fail | Inspect the code and confirm or dismiss a candidate finding |
| uncertain | Gather the context needed to decide |
| pass | Spend less attention on this contract/file pair |
| not_applicable | Usually skip this pair |
| error or incomplete | Retry when appropriate or report missing coverage |

For a database-error finding, read the contract, function, caller, and diff. Check whether an exception applies and whether the PR introduced the behavior.
A model failure is a candidate finding, not a ready-to-publish review comment.
Architecture questions may require imports, dependency metadata, or multiple files that Tenet did not inspect.

The harness owns investigation notes and the final review. It does not call `tenet record`, maintain a Tenet review ID, or expose dedicated Tenet tools.
It should also review concerns outside the available contracts.
