# Reviews

Tenet supplies fast, model-based checks and remembers what each contract concluded for the exact code it saw.
The reviewing agent reads the contracts, investigates the code, runs appropriate project checks, records a
conclusion for each pending contract, and writes the final review.

```sh
tenet check --base origin/main
tenet status
tenet show database-failures
tenet record database-failures --conclusion failed \
  --evidence src/storage/query.ts:40-52 -m "query errors become an empty list"
```

`check` captures the change, asks the model, and appends revision `rN` to `.tenet/events.jsonl`. `status` recomputes
each contract's input from the working tree and reports it settled or pending without calling a model. A model's
favorable conclusion settles a contract; its `failed` or `unresolved` stays pending until someone records a conclusion.
See [review states](./output.md#review-states).

`show` prints what a reviewer needs for one contract: the model's answer and confidence, the scoped files, earlier
records and assessments, the changes since an outdated one, the `record` command with the conclusions that fit, the
complete contract, and the change. Read every contract. A favorable assessment can save time; it does not remove the
contract from the review. Confirm suspected violations in the code before recording them.

A record cites line ranges of the current files and a message saying what they show. Record `unresolved` with a
message when you cannot decide; it exits 3 instead of blocking. Exit 0 means every contract is settled without a
violation. Read each claim's scope: selected subjects can be `clear` without verifying a whole contract.

## Keep reviews current

Every conclusion is bound to its input. Editing a scoped file makes its contract pending with `inputs_changed`, and
`record` refuses it until the next `check`. Then `show` lists the earlier record as outdated with a diff of what
changed since. Reverting the edit restores the earlier input, so its record settles the contract again.

Repeated checks answer identical requests from `.tenet/answers.jsonl`, so an unchanged contract costs no request and
keeps its answer. Use `--fresh` to sample the model again. `tenet check --no-model` captures a revision without
credentials; only conclusions for unchanged inputs carry over. `tenet log` lists every revision and record.

An agent can split the work: give one subagent each pending contract with `tenet show NAME`, and have it record a
conclusion. Records for different contracts do not conflict.

To keep an agent working until every contract is settled, run `tenet status` when it tries to stop. In Claude Code,
a `Stop` hook that exits 2 sends its stderr back to the agent:

```json
{
  "hooks": {
    "Stop": [
      { "hooks": [{ "type": "command", "command": "tenet status >&2; [ $? -ne 4 ] || exit 2" }] }
    ]
  }
}
```

The hook blocks only on exit 4. Without a `.tenet/` directory, `status` exits 2 and the agent can stop.

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

Use [`tenet export`](./export.md) to export selected contracts and source without calling a model. A saved packet
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
