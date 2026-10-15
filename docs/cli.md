# Commands

```sh
tenet check
tenet check --base origin/main
tenet status [--base REF] [--json]
tenet show NAME [PATH ...] [--json]
tenet show --pending
tenet record NAME [PATH ...] --conclusion CONCLUSION [--evidence PATH:START-END ...] -m MESSAGE
tenet log [NAME] [--json]
tenet export src/ --base origin/main > evidence.json
tenet validate
tenet contracts list [--for FILE] [--base REF] [--json]
tenet contracts show NAME
tenet contracts add SOURCE/NAME[@REF] (--scope DIR | --path FILE)
tenet contracts outdated | update | diff PATH | remove PATH
```

Use `--root DIR` to choose a project; the default is the working directory. Contracts outside that root do not participate.

## Full check

```sh
tenet check
tenet check --contract cache-values
```

Assess the current repository without assuming that any contract holds. File judgments are followed by a bounded repository assessment to check missing files and requirements that span files.

`tenet check src/` checks those paths together with explicit context. Coverage is calculated for each contract.
Selecting every file in a nested contract can verify it. A smaller selection can conclude `clear` or `failed`;
`clear` means no violation was found in the selected subjects and does not verify the full contract scope.
Contracts in unrelated directories do not participate unless named with `--contract`, and neither do contracts whose
`paths` match none of the selected files. An empty contract directory
still receives a full assessment when you select that directory or an ancestor, so missing required files remain
checkable. Naming a contract does not broaden any positional selection supplied with it.

## Diff check

```sh
tenet check --base origin/main
tenet check --base HEAD --dry-run --json
```

Assume the base satisfies the selected contracts, then assess whether the change preserves them. `--base REF` compares that exact commit with the working tree, including staged, unstaged, and untracked files. It does not choose a merge base automatically.

In CI, pass the merge base, not `origin/main`; otherwise commits that reached `main` after the branch started count
as part of the change. The merge base needs history back to that point, so check out full history, such as
`fetch-depth: 0` in `actions/checkout`, or unshallow first:

```sh
git fetch --unshallow origin main 2>/dev/null || git fetch origin main
tenet check --base "$(git merge-base origin/main HEAD)"
```

Relevance and violation questions share one request: relevance concerns the before/after change, including deletions
and renames, while violation asks whether the change introduces, extends, or worsens a violation compared with the
before contents. Another instance or a copy of an existing violation counts, and so does one created by moving,
reordering, or deleting code. A violation the base already had, which the change keeps without adding to it, does not
fail the diff, even in an edited file; only a full check reports it. The final combined assessment asks the same
question. It retains all captured selected source in the contract scope, regardless of relevance answers, plus before
contents, patches, explicit context, and the file inventory. An unrelated change can leave a contract unaffected;
relevant changes need evidence of preservation. A diff that changes no file a contract governs, and not the contract
itself, concludes `unaffected` without a model request.
The caller owns establishing the baseline and running full checks when contracts change. Tenet does not silently switch modes.

`contracts list --base REF --json` returns the containing contract scopes in evaluation order, plus changed contract paths.
It needs no model credentials. New and old paths participate, including deletions and moves.
Diff discovery validates contracts in those scopes. Malformed contracts in unrelated folders do not block the diff;
`validate` still checks every contract, and selecting a contract by name requires repository-wide discovery.

## Snapshot and patch

```sh
tenet check --snapshot ./base
tenet check --snapshot ./base --patch ./diff/get-or-set.patch
```

Copy the snapshot into a temporary repository, optionally apply a Git-compatible patch, and assess the result. The original directory stays unchanged. Temporary files are removed on exit.
If the snapshot has no `.contracts`, Tenet copies `.contracts` from its parent directory. Paths are relative to the snapshot's root; the patch uses ordinary `a/` and `b/` prefixes.

Without a patch this is a full check. With a patch it is a diff check. `--base` and `--snapshot` cannot be combined.

## Review state

Each `check` outside a snapshot appends a revision to `.tenet/events.jsonl` with every contract's input and conclusion,
unless it repeats the latest revision. A revision lists each captured file once, and in a diff, the lines the change wrote. Tenet still reads logs
that 0.8.0 wrote and appends the current format to them. Checks against the same base commit form one review; `status` compares
the working tree with that commit, not with what `--base` names today.
The directory ignores itself; if Git tracks a file in it anyway, every command below exits 2 until it is untracked.
Each contract is settled or pending; see [Reviews](./reviews.md).

| Command | Effect | Exit |
| --- | --- | --- |
| `status` | Recompute inputs from the working tree and list each contract's state; never writes or calls a model | As `check` |
| `status --base REF` | The same for the review against `REF` | As `check`, or 4 when no check compares with `REF` |
| `show NAME` | Show the model's answer, records, changes since an outdated result, the contract, and the change | 0 |
| `show --pending` | Show every pending contract and their shared change once | 0 |
| `record NAME` | Settle the contract's current input with a conclusion, evidence, and a message | 0, or 2 if refused |
| `log [NAME]` | List revisions, assessments, and records, newest first; `--json` prints each entry in order | 0 |

`record` refuses a contract whose scoped files changed after its latest check: run `check` again first. The conclusion
must fit the check: `verified` for a full check of the full contract, `preserved` or `unaffected` for a diff, and `clear`
for selected subjects, plus `failed` and `unresolved` everywhere. `--evidence` cites lines of files the check assessed,
under the same path rules as `--context`; it is required unless the conclusion is `unresolved`. In a diff, a favorable
conclusion must cite a changed line, a line beside one, or a file the change deleted or emptied; when the diff changed none of the
contract's files, it needs no evidence but requires a `verified` full check of the same contract text. A favorable
conclusion over the model's `failed` must cite a file the model flagged. The message is plain text up to 4 KiB without
control, bidirectional, or line separator characters. `show` prints the command with the conclusions and changed lines
that fit. `record` names the record it replaces when the conclusions differ.

When a contract stands in several checks of the review, such as a check of the whole change and a later check of one
file, `status` lists it once per check. Pass that check's paths after the name, as in `tenet show errors README.md`.

`show --json` and each `log --json` line carry `version: 2`. `show --json` lists every scoped file, the latest five
records and earlier assessments, and counts of the older ones.

`check --no-model` captures inputs and appends a revision without credentials or requests. Contracts settled for the
same input stay settled; the rest are pending. Each row shows the reused model conclusion, as in
`✓ errors (1 file) preserved · recorded 2540a6592efb over model failed · violation 100% · unchanged`. A model check
answers repeated identical requests from `.tenet/answers.jsonl`; `--fresh` asks the model again. Delete `.tenet/` to
forget the review.

## Export

`tenet export` exports selected source and complete contract documents without model requests. It accepts the same
input selection as `check` and writes one JSON object. See [Export](./export.md) for source identities, scope,
size limits, and examples of preparing a focused question.

## Shared contracts

These commands install contracts from a library and record them in `.tenet.lock` at the project root. They are the only
commands that read the lock or reach the network, and they need `git`. `check`, `export`, and `contracts list` and
`show` never read the lock; `validate` checks its format and that each recorded file exists. See
[Guidance](./guidance.md) for the workflow.

| Command | Effect |
| --- | --- |
| `add SOURCE/NAME[@REF] --scope DIR` | Copy a library contract into the next numbered folder under `DIR/.contracts` |
| `add SOURCE/NAME[@REF] --path FILE` | Record an existing copy; its differences count as local edits |
| `outdated [--offline] [--json]` | Show each copy's release, newest release, and local state |
| `update [PATH ...] [--ref REF] [--json]` | Replace unedited copies, merge edited ones, and move the lock |
| `diff PATH [--json]` | Show upstream changes since the recorded release, then local edits |
| `remove PATH [--keep-file]` | Delete a copy and its lock entry, or keep the file as a local contract |

`SOURCE` is `tenet` for https://github.com/macromackie/tenet-contracts. Register another Git library, or a local
clone of that one, under another alias with `--source ALIAS=URL` on `add`; Git uses your existing credentials.
Without `@REF`, `add` takes the highest `vX.Y.Z` tag.
`add` also accepts `--name LOCAL` and `--number N`. `update` refuses uncommitted changes to the lock or selected
contracts, so every merge is a reviewable diff.

| Exit | `add`, `diff`, `remove` | `outdated` | `update` |
| --- | --- | --- | --- |
| 0 | Done | Every copy is current and present | Every copy is unchanged, replaced, or merged |
| 1 | Not used | A copy changed or was removed upstream, or is missing | A conflict was written or a contract was removed upstream |
| 2 | Invalid input or execution error | Same | Same |

## Check options

| Option | Default | Purpose |
| --- | --- | --- |
| `--contract NAME` | all contracts | Select one requirement |
| `--context PATH` | none | Add a repository file to check inputs; repeat for multiple files |
| `--jobs N` | 4 | Maximum simultaneous model requests |
| `--max-requests N` | 1000 | Request budget, including combined assessments |
| `--min-confidence N` | 0.8 | Route lower-confidence answers to unresolved; accepts 0 through 1 |
| `--provider NAME` | openrouter | Select provider |
| `--model ID` | provider default | Select a supported pinned model |
| `--reporter KIND` | default | `default`, `verbose`, or `jsonl` |
| `--json` | off | Shorthand for JSONL |
| `--no-model` | off | Capture and keep a revision without requests; see [Review state](#review-state) |
| `--fresh` | off | Ask the model again instead of reusing answers to identical requests |

`check --dry-run` lists candidate pairs and builds each file input, including the contract and explicit context,
without credentials or model requests. JSON `selected` records include encoded `input_bytes`, `input_limit_bytes`,
`omitted` parts, and a `status` and `reason` for files that would be skipped or incomplete. A `combined` record per
contract gives its repository `context`, including its size and any reductions. The summary predicts `requests`, the
file and combined requests a check would send before reusing earlier answers, and counts `combined_over_limit`, the
contracts whose combined input exceeds the limit after reduction. A prediction above `--max-requests` prints a warning
to stderr. A diff with no changes prints `No changes since BASE; nothing to check.` Only execution errors exit 2.
`validate` and `contracts` also work without credentials.

## Discovery

The inventory respects Git ignore files and `.ignore`. Tool configuration and other hidden files are included.
Only `.git`, `.tenet`, `.tenet.lock`, `.env`, and `.env.*` are excluded by name, at any depth. A directory with its own
`.git`, such as a nested repository or a worktree under `.claude/worktrees/`, is a separate project and is skipped.
List build output such as `target/`, `node_modules/`, or `dist/` in `.gitignore` or `.ignore` to keep it out.
Files inside `.contracts` folders are not subjects unless a contract's [`paths`](./contracts.md#scope) name them.
Tracked changes to ordinary files are assessed even if current ignore rules hide them from the inventory; a `!` pattern
in a contract's [`paths`](./contracts.md#scope) leaves such a file out. Never store credentials in files you submit for assessment.

When a selected path yields nothing to assess, `check` and `export` print a warning to stderr and continue.
Either discovery found no files there, because ignore files or excluded names hide them or the directory is empty,
or no selected contract governs the files found.

Symlinks are not followed. Symbolic links and empty, binary, or non-UTF-8 files are skipped: they stay in the inventory, but no model judges their contents. Unreadable inputs are errors.
There is no configuration file, stored baseline, generated verifier, or automatic fixer.
