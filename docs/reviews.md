# Reviews

The reviewing agent owns when to establish or refresh the baseline:

```sh
# First adoption
tenet check --reporter jsonl

# New or changed requirement
tenet check --contract cache-values --reporter jsonl

# Routine code review, assuming the base satisfies the contracts
tenet check --base origin/main --reporter jsonl
```

Capture stdout and exit status even when the command returns nonzero.
Focus investigation on failed and unresolved contracts. Preserved and unaffected conclusions rely on the caller's baseline assumption.
A failed model judgment is a candidate finding; inspect the code before publishing a review comment.

Each report identifies inputs with hashes. Rerun after changing source or contracts; keep the checkout stable during a run.
An unresolved conclusion names a coverage limit, unavailable context, low confidence, or operational failure. Use ordinary repository tools to investigate it.

Tenet has no internal agent loop, persistent review ID, or automatic LLM escalation. The harness owns follow-up, baseline validity, notes, and the final review, including concerns outside the contracts.
