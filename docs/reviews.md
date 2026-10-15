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

`--base` is resolved once. The review stays on that commit, so committing the change or moving `origin/main` does not
settle or drop anything. `status` names the commit in its first line and says when the working tree has no changes.
`status --base REF` reports the review against that commit instead of the latest check's, and exits 4 when no check
compares the working tree with it.

`.tenet/` ignores itself, and its records and cached answers describe one machine's review. If Git tracks a file in
it, `check`, `status`, `show`, `record`, and `log` exit 2 until you untrack it with `git rm -r --cached .tenet`.

`show` prints what a reviewer needs for one contract: the model's answer and confidence, the scoped files, earlier
records and assessments, the changes since an outdated one, the `record` command with the conclusions and changed lines
that fit, the complete contract, and the change. For a scope over 20 files, it counts files and file results by kind and
lists only those that need attention, naming five unreadable files and counting the rest; `show --json` lists every
file. It lists the latest five records and assessments, and `tenet log NAME` lists the rest. `show --pending` prints
every pending contract this way and their shared change once. An unresolved row shows which way the model leaned, as
in `leans no violation · confidence 0.56 < 0.8`. Read every contract. A favorable assessment can save time; it does not remove the
contract from the review. Confirm suspected violations in the code before recording them.

A record cites line ranges in files the check assessed, the selected files and any `--context` files, and a message
saying what they show. Evidence follows the same path rules as `--context`, so `.git`, `.tenet`, contract documents, and
links that lead out of the project are refused. In a diff that changed the contract's files, a favorable conclusion
must cite a line the change wrote, a line beside one, or a file it deleted or emptied; code near the change does not show it was
preserved. To dispute a model's `failed`, also cite at least one file it flagged. Messages are plain text up to 4 KiB,
without control, bidirectional, or line separator characters. Record `unresolved` with a message when you cannot decide;
it exits 3 instead of blocking. Exit 0 means every contract is settled without a violation. Read each claim's scope: selected subjects can
be `clear` without verifying a whole contract.

A record that overrides the model stays visible. `status` and `check` print it as
`✓ preserved · recorded 2540a6592efb over model failed · violation 100%` and count contracts settled by a record
separately from those the model settled. The latest record for an input decides it; `record` names the record it
replaces when their conclusions differ. Tenet checks each record again whenever it reads the log; a line that `record`
would refuse, such as an edited conclusion, is an error naming that line. Records that Tenet 0.8.0 wrote are checked
only for the revision and input they name. Tenet never edits the log: move `.tenet` aside, run `check`, and record the
review again.

## Keep reviews current

Every conclusion is bound to its input. Editing a scoped file makes its contract pending with `inputs_changed`, and
`record` refuses it until the next `check`. Then `show` lists the earlier record as outdated with a diff of what
changed since. Reverting the edit restores the earlier input, so its record settles the contract again.
`.tenet/blobs` keeps captured files up to 1 MiB for these diffs; for a larger file, `show` notes that its contents
were not kept.

Repeated checks answer identical requests from `.tenet/answers.jsonl`, so an unchanged contract costs no request and
keeps its answer. Use `--fresh` to sample the model again. `tenet check --no-model` captures a revision without
credentials; only conclusions for unchanged inputs carry over, and its rows mark them `unchanged`. `tenet log` lists
every revision and record. Each revision keeps its `--min-confidence`, and `status`, `show`, and `log` print it when it
differs from the default 0.8.

Checks against the same base commit form one review, whichever paths they select. A later check replaces an earlier
assessment of a contract only when it selects every path the earlier check did, or the contract's whole scope.
Otherwise both stand, and the contract is settled only when each is. A check of one file therefore cannot hide a
failure that a check of the whole change found. In a diff review, a check that names one contract with `--contract`
leaves the change's other contracts `not_checked` rather than dropping them. `status` lists such a contract once per standing check and notes the
earlier check's paths; `tenet show NAME PATH...` and `tenet record NAME PATH...` name the one you mean.

An agent can split the work: give one subagent each pending contract with `tenet show NAME`, and have it record a
conclusion. Records for different contracts do not conflict.

To remind an agent to settle every contract before it stops, report its review when it tries to stop. In Claude Code,
a `SessionStart` hook can pin the commit the session started from, and a `Stop` hook that exits 2 sends its stderr
back to the agent. Save the scripts in `.claude/hooks/`, make them executable, and commit them with the settings;
uncommitted hook files would join every session's change:

```sh
#!/usr/bin/env bash
# tenet-start.sh: review everything the session changes from here.
cd "$CLAUDE_PROJECT_DIR" && git_dir=$(git rev-parse --git-dir) && base=$(git rev-parse HEAD) || exit 0
echo "$base" > "$git_dir/tenet-session-base"
tenet check --base "$base" --no-model >/dev/null 2>&1
exit 0
```

```sh
#!/usr/bin/env bash
# tenet-stop.sh: send the agent back once per stop while the review is pending or unreadable.
grep -Eq '"stop_hook_active"[[:space:]]*:[[:space:]]*true' && exit 0
cd "$CLAUDE_PROJECT_DIR" && git_dir=$(git rev-parse --git-dir) || exit 0
base=$(cat "$git_dir/tenet-session-base") || exit 0
status=$(tenet status --base "$base" 2>&1)
case $? in 2 | 4) ;; *) exit 0 ;; esac
printf 'The Tenet review against %s is not settled. Run tenet check --base %s, then settle each\n' "$base" "$base" >&2
printf 'pending contract with tenet show --pending and tenet record. Record unresolved if you cannot decide.\n\n%s\n' "$status" >&2
exit 2
```

```json
{
  "hooks": {
    "SessionStart": [
      { "matcher": "startup", "hooks": [{ "type": "command", "command": "\"$CLAUDE_PROJECT_DIR\"/.claude/hooks/tenet-start.sh" }] }
    ],
    "Stop": [
      { "hooks": [{ "type": "command", "command": "\"$CLAUDE_PROJECT_DIR\"/.claude/hooks/tenet-stop.sh" }] }
    ]
  }
}
```

The pinned base keeps commits made during the session in the review. The start hook's `--no-model` check opens that
review without requests, so `status --base` covers every contract the session's change touches: a contract no model
has checked is pending as `not_checked`. `status` reads the review without writing it or calling a model, and its exit
code decides the hook:

| Exit | Meaning for the hook |
| --- | --- |
| 0 | Every contract is settled without a violation; let the agent stop |
| 1 | A recorded failure; the review is complete, and the agent reports the violation |
| 3 | A recorded `unresolved` conclusion; the review is complete, and the agent reports what is missing |
| 4 | A contract is pending, or no check compares the working tree with the session base; block |
| 2 | Tenet could not read the review, for example after `.tenet/` was deleted, edited, or committed; block |

Claude Code sets `stop_hook_active` in a `Stop` hook's input when the agent is already continuing because a stop hook
blocked. The recipe then lets it stop, so it blocks once per stop and cannot trap an agent that cannot settle a contract.

The hook is a reminder, not enforcement. The agent can edit the hooks, the pinned base, and `.tenet/`, and the hook
believes what it reads there. Enforce contracts in CI, which starts without `.tenet/` and checks the change itself; see
[full checks after an update](./guidance.md#full-checks-after-an-update).

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

When the full check finds violations already in the code, choose for each contract: fix them, narrow the contract with
`paths` or a `## Local adaptations` exception, or record the failure and keep reviewing changes. A diff check fails a
violation the base already has only when the change adds to it, so new work stays reviewable while the debt remains.
Run the full check again once the debt is fixed.

When a diff adds or edits a contract, the diff check's assumption that the base complied with it no longer holds.
A favorable answer leaves that contract pending as `contract_changed`. Check the whole contract with
`tenet check --contract NAME`, which starts a separate full review, then rerun the diff check and record a conclusion.
When a diff changes a contract but no file it governs, the check selects nothing for it, so a favorable record cites
no lines. `record` instead requires the full check of the same contract text to be settled as `verified`.
`check` and `status` list contract documents the diff deletes, including one moved to a path that is not a contract,
and `contracts list --base ... --json` includes every changed contract path. Inspect those changes; a removed
requirement leaves the review with its contract.

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
When the input is too large, Tenet [reduces it](./limits.md) where patches show the change; if it still does not fit,
the file checks decide alone and a clean result stays unresolved. Narrow positional paths or supporting
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
