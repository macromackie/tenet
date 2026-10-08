# Reviews

Tenet supplies fast, model-based checks. The reviewing agent reads the contracts, investigates the code,
runs appropriate project checks, and writes the final review.

```sh
tenet contracts list --base origin/main --json
tenet contracts view database-failures
tenet check --base origin/main --json > report.jsonl
```

Read every contract in the inventory. Use failed, unresolved, and lower-confidence assessments to decide where
to investigate first. A favorable assessment can save time; it does not remove the contract from the review.
Confirm suspected violations before commenting on code. Exit 0 means the requested claims completed without violations. Read each claim's scope: selected subjects can be
`clear` without verifying a whole contract. Unresolved results exit 3.

## Add context

If a helper owns the error handling, supply it and rerun the relevant contract:

```sh
tenet check --base origin/main --contract database-failures \
  --context src/storage/query.ts --json
```

Context files supplement the input; they do not expand the contract's scope. Tenet retains their before/after identity.
Keep the checkout stable during a run. Tenet captures selected and supporting source before model assessment, then
reuses those bytes throughout the run. Capture is not an atomic filesystem snapshot. Capture stdout and exit status
even when the command returns nonzero.

Use [`tenet evidence`](./evidence.md) to export selected contracts and source without calling a model. A saved packet
can supply a focused question or a description of the change, with references back to each before/after source.
Descriptions help choose what to inspect; verify their claims against the source and the full English contracts.

## Establish the baseline

A diff check assumes the base satisfies its contracts. On first adoption, run a full check. New or changed contracts
also need a full check of their scope:

```sh
tenet check --json
tenet check --contract database-failures --json
```

`contracts list --base ... --json` includes changed contract paths, including deleted ones. Inspect those changes;
a removed or weakened requirement must not silently disappear from the review.

The outer agent owns baseline validity, follow-up, and concerns outside the contracts. Tenet has no internal agent loop
or automatic escalation to a larger model.

## Target a question

The default check selects files within each contract's scope. Tenet does not generate AST matchers.
An agent can use ast-grep and ev-grep when a narrower question helps:

```sh
ast-grep run --lang javascript --pattern 'function $NAME($$$ARGS) { $$$BODY }' --json=stream src/ \
  | ev-grep 'Catches an error and returns an empty result' --candidates - --json
```

This pattern selects function declarations, not methods or arrow functions. A match helps locate code to read;
a lack of matches does not establish that the contract holds. Use the full inventory for folder requirements
and trace callers for rules spanning files.

## Scheduling and context

Tenet checks independent contracts concurrently within `--jobs`, then prints their summaries in contract order.
For each file it asks relevance and violation questions in one Jev request. The request includes the before/after
change and resulting source. Relevance is a hint for the reviewer's attention; it does not remove selected source
from the contract's evidence or override a violation routed to `match`.

The final assessment receives all captured selected source within the contract scope, explicit `--context` inputs,
and the scoped file inventory. Tenet attempts this bounded combined assessment for selected-path checks too.
If the complete input exceeds its limit, the contract remains unresolved. Narrow positional paths or supporting
context explicitly when appropriate, and account for what the smaller selection leaves unreviewed.
Tenet does not fill spare space with unrelated source. If a file check and final assessment disagree about a violation,
Tenet reports `unresolved` with `conflicting_files`. The reviewer decides whether more context or a code change is needed.

Request metadata includes attempts, hedges, elapsed time, and available usage. Recovered transport failures do not
change the meaning of a model answer. Unresolved assessments remain guidance; execution errors remain errors.

## Agent skill

Download the [tenet skill](https://tenet-contracts.com/skill.md) into your agent's project skill directory. For agents that read `.agents/skills`:

```sh
mkdir -p .agents/skills/tenet
curl -fsSL https://tenet-contracts.com/skill.md -o .agents/skills/tenet/SKILL.md
```

Inspect the downloaded instructions before adopting them. The skill covers input selection, interpreting results,
and investigating findings. It also describes a focused curation loop for authorized code and contract changes.
Local code-curation and architecture-curation skills can supply project conventions; they are optional.

For code and contract maintenance workflows, see [Guidance](./guidance.md).
