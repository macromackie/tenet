# Commands

```sh
tenet check
tenet check --base origin/main
tenet status [--base REF] [--json] [--prune-cache]
tenet show NAME [PATH ...] [--json]
tenet show --pending [--json]
tenet record NAME [PATH ...] --conclusion CONCLUSION [--evidence PATH:START-END ...] -m MESSAGE
tenet log [NAME] [--json]
tenet export src/ --base origin/main > evidence.json
tenet validate
tenet contracts list [--for FILE] [--base REF] [--json]
tenet contracts show NAME
tenet contracts add SOURCE/NAME[@REF] (--scope DIR | --path FILE)
tenet contracts outdated | update | diff PATH | remove PATH
tenet skills add SOURCE/NAME[@REF] [--dir DIR | --path DIR]
tenet skills outdated | update | diff PATH | remove PATH
```

Use `--root DIR` to choose a project; the default is the working directory. Contracts outside that root do not participate.
Use `--state-dir DIR`, or `TENET_STATE_DIR`, to keep [review state](#review-state) outside a read-only
checkout; every review command must name the same directory.
Use `--trace PATH`, or `TENET_TRACE`, to write a JSON Lines trace of spans and events, or `--trace -` for stderr.
The trace records failure kinds, statuses, counts, and durations, never contract text, file contents, keys, or provider
messages. A check's `tenet.check` span takes the caller's trace and span ids from a W3C `TRACEPARENT`.
Inside it, `tenet.contract` and `tenet.pair` spans cover each contract and each contract and file.
`tenet.wait` times each wait for a parallel slot.
`cache.hit` and `budget.exhausted` events mark answers read from the cache and requests the budget refuses.

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

`contracts list --base REF --json` returns the containing contract scopes in evaluation order, plus changed contract paths
and the change's [`coverage`](./output.md#coverage): the changed files no contract governs. A change to any file in a
contract's folder, such as an installed asset, counts as a change to that contract.
It needs no model credentials. New and old paths participate, including deletions and moves.
When no contract governs any changed file, `check` says `No contract governs this change: N changed files` and exits 0.
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

| Command                | Effect                                                                                                       | Exit                                               |
| ---------------------- | ------------------------------------------------------------------------------------------------------------ | -------------------------------------------------- |
| `status`               | Recompute inputs from the working tree and list each contract's state; never writes or calls a model         | As `check`                                         |
| `status --base REF`    | The same for the review against `REF`                                                                        | As `check`, or 4 when no check compares with `REF` |
| `status --prune-cache` | First remove the cached answers this Tenet cannot reuse, and say how many on stderr; the log stays unchanged | As `status`                                        |
| `show NAME`            | Show the model's answer, records, changes since an outdated result, the contract, and the change             | 0                                                  |
| `show --pending`       | Show every pending contract and their shared change once                                                     | 0                                                  |
| `record NAME`          | Settle the contract's current input with a conclusion, evidence, and a message                               | 0, or 2 if refused                                 |
| `log [NAME]`           | List revisions, assessments, and records, newest first; `--json` prints each entry in order                  | 0                                                  |

`record` refuses a contract whose scoped files changed after its latest check: run `check` again first. The conclusion
must fit the check: `verified` for a full check of the full contract, `preserved` or `unaffected` for a diff, and `clear`
for selected subjects, plus `failed` and `unresolved` everywhere. `--evidence` cites lines of files the check assessed,
under the same path rules as `--context`, and only the supplied lines of a supporting `--context` excerpt; it is required unless
the conclusion is `unresolved`. In a diff, a favorable
conclusion must cite a changed line, a line beside one, or a file the change deleted or emptied; when the diff changed none of the
contract's files, it needs no evidence but requires a `verified` full check of the same contract text. A favorable
conclusion over the model's `failed` must cite a file the model flagged. The message is plain text up to 4 KiB without
control, bidirectional, or line separator characters. `show` prints the command with the conclusions and changed lines
that fit. `record` names the record it replaces when the conclusions differ.

When a contract stands in several checks of the review, such as a check of the whole change and a later check of one
file, `status` lists it once per check. Pass that check's paths after the name, as in `tenet show errors README.md`.

`show --json` and each `log --json` line carry `version: 2`. `show --json` lists every scoped file, the latest five
records and earlier assessments, and counts of the older ones. `show --pending --json` lists every pending contract in
that form and each changed file's patch once. See [JSON](./json.md) for every command's format.

`check --no-model` captures inputs and appends a revision without credentials or requests. Contracts settled for the
same input stay settled; the rest are pending. Each row shows the reused model conclusion, as in
`✓ errors (1 file) preserved · recorded 2540a6592efb over model failed · violation 100% · unchanged`. A model check
answers repeated identical requests from `.tenet/answers.jsonl`; `--fresh` asks the model again. An answer is reused
only by the same Tenet version and provider, for the model revision the provider reported. `status --prune-cache`
removes the answers an earlier Tenet wrote and those a later answer to the same request replaced. Delete `.tenet/` to
forget the review.

## Export

`tenet export` exports selected source and complete contract documents without model requests. It accepts the same
input selection as `check` and writes one JSON object. See [Export](./export.md) for source identities, scope,
size limits, and examples of preparing a focused question.

## Shared contracts and skills

`tenet contracts` and `tenet skills` install contracts and agent skills from a library and record them in `.tenet.lock`
at the project root. They are the only commands that write the lock or reach the network. They need `git`, and `add`,
`update`, and `remove` need the project to be a Git checkout. `check`, `export`, and `contracts list` and `show` read
only the lock's [`projects`](#discovery). `validate` checks offline that the lock is well formed, that each recorded
file exists, and that each read-only file matches its hash. See [Guidance](./guidance.md) for the workflow.

| Command                                       | Effect                                                                                                |
| --------------------------------------------- | ----------------------------------------------------------------------------------------------------- |
| `contracts add SOURCE/NAME[@REF] --scope DIR` | Copy a library contract and its assets into the next numbered folder under `DIR/.contracts`           |
| `contracts add SOURCE/NAME[@REF] --path FILE` | Record an existing copy; its differences count as local edits                                         |
| `skills add SOURCE/NAME[@REF] [--dir DIR]`    | Copy a library skill's folder into `DIR/NAME`; `DIR` defaults to `.agents/skills`                     |
| `skills add SOURCE/NAME[@REF] --path DIR`     | Record an existing copy; its Markdown differences count as local edits                                |
| `outdated [--offline] [--json]`               | Show each copy's release, newest release, majors crossed, and local state, and each file that changed |
| `update [PATH ...] [--ref REF] [--json]`      | Replace unedited files, merge edited Markdown, and move the lock, at most to the next major           |
| `diff PATH [--json]`                          | Show upstream changes since the recorded release, then local edits, for each file that differs        |
| `remove PATH [--keep-file]`                   | Delete a copy's locked files and its lock entry, or keep them as local files                          |

`SOURCE` is `tenet` for <https://github.com/macromackie/tenet-contracts>. Register another Git library, or a local
clone of that one, under another alias with `--source ALIAS=URL` on `add`; Git uses your existing credentials.
Without `@REF`, `add` takes the highest `vX.Y.Z` tag. Without `--ref`, `update` moves to the highest tag of the next
major, or of the recorded major when no later one exists; `--ref` names any release, later or earlier. Both `add`
commands accept `--name LOCAL`, which also rewrites the copy's `name`; `contracts add` accepts `--number N`.
`skills add` refuses a destination that exists. `add` and `update` refuse, before writing anything, a file that Git
ignores, a library contract file other than `CONTRACT.md` and `assets/<tool>/` files, and a release that needs a
newer Tenet. A skill's `PATH` is its folder; a contract's is its `CONTRACT.md` or folder.

`CONTRACT.md`, `SKILL.md`, and a skill's other Markdown files merge three ways. Every other locked file, such as a
contract's assets or a skill's scripts, is read-only. `update` replaces it while unedited and refuses, before writing
anything, while one in a selected copy is edited or missing. It also refuses uncommitted changes to the lock or the
selected copies, so every merge is a reviewable diff, and a local file in the way of one upstream adds. A file removed
upstream stays in place and leaves the lock. `remove` refuses uncommitted changes to the files it would delete. Tenet
never touches files the lock does not list. Hashes ignore the `name` value of `CONTRACT.md` and `SKILL.md`. Hashes of
assets and skill files read CRLF line endings as LF; a contract's own hash covers its exact bytes, as in version 1.

| Exit | `add`, `diff`, `remove`          | `outdated`                                                                                           | `update`                                                       |
| ---- | -------------------------------- | ---------------------------------------------------------------------------------------------------- | -------------------------------------------------------------- |
| 0    | Done                             | Every copy is current and present                                                                    | Every file is unchanged, replaced, added, or merged            |
| 1    | Not used                         | A file changed, was added, or was removed upstream, a file is missing, or a read-only file is edited | A conflict was written, or a copy or file was removed upstream |
| 2    | Invalid input or execution error | Same                                                                                                 | Same, including a refused update                               |

The lock is written as version 3 when it lists `projects`, as version 2 when it records skills or contract assets, and
as version 1 otherwise. A Tenet older than 0.11.0 cannot read a version 2 lock, and Tenet 0.11.0 cannot read version 3.

## Check options

| Option                       | Default          | Purpose                                                                                                                                                   |
| ---------------------------- | ---------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `--contract NAME`            | all contracts    | Select one requirement                                                                                                                                    |
| `--context PATH[:START-END]` | none             | Add a repository file, or only its lines START through END, to check inputs; repeat for more files or ranges; see [Add context](./reviews.md#add-context) |
| `--jobs N`                   | 4                | Maximum simultaneous model requests                                                                                                                       |
| `--max-requests N`           | 1000             | Request budget, including combined assessments                                                                                                            |
| `--min-confidence N`         | 0.8              | Route lower-confidence answers to unresolved; accepts 0 through 1                                                                                         |
| `--provider NAME`            | openrouter       | Select provider                                                                                                                                           |
| `--model ID`                 | provider default | Select a supported pinned model                                                                                                                           |
| `--reporter KIND`            | default          | `default`, `verbose`, or `jsonl`                                                                                                                          |
| `--json`                     | off              | Shorthand for JSONL                                                                                                                                       |
| `--no-model`                 | off              | Capture and keep a revision without requests; see [Review state](#review-state)                                                                           |
| `--fresh`                    | off              | Ask the model again instead of reusing answers to identical requests; with `--dry-run`, predict as if nothing were cached                                 |

`check --dry-run` lists candidate pairs and builds each file input, including the contract and explicit context,
without credentials or model requests. JSON `selected` records include encoded `input_bytes`, `input_limit_bytes`,
`omitted` parts, a renamed file's `previous_path`, and a `status` and `reason` for files that would be skipped or
incomplete. A `combined` record per contract gives its repository `context`, including its size and any reductions.
The summary counts `requests`, the file and combined requests a check needs, and `cached`, how many of them earlier
answers in `.tenet/answers.jsonl` would answer; a check sends the rest. With `--fresh`, it predicts as if nothing were
cached. It also counts `combined_over_limit`, the contracts whose combined input exceeds the limit after reduction, and
reports the diff's `coverage`. When the requests to send exceed `--max-requests`, it prints a warning to stderr. A diff with no changes
prints `No changes since BASE; nothing to check.` Only execution errors exit 2.
`validate`, `contracts`, and `skills` also work without credentials.

## Discovery

The inventory respects Git ignore files and `.ignore`. Tool configuration and other hidden files are included.
Only `.git`, `.tenet`, `.tenet.lock`, `.env`, and `.env.*` are excluded by name, at any depth. A directory with its own
`.git`, such as a nested repository or a worktree under `.claude/worktrees/`, is a separate project and is skipped.
So is each directory that matches a glob pattern in the `projects` list of the root `.tenet.lock`, such as test
fixtures with contracts of their own. Add the list by hand and set the lock's `version` to 3; `add`, `update`, and
`remove` keep it:

```json
"projects": ["tests/fixtures/*/project"]
```

Patterns are relative to the project root, and `*` stays within one path segment. A diff skips changes inside those
directories too. Run `tenet --root DIR` to validate or check one of them.

List build output such as `target/`, `node_modules/`, or `dist/` in `.gitignore` or `.ignore` to keep it out.
Files inside `.contracts` folders are not subjects unless a contract's [`paths`](./contracts.md#scope) name them.
Tracked changes to ordinary files are assessed even if current ignore rules hide them from the inventory; a `!` pattern
in a contract's [`paths`](./contracts.md#scope) leaves such a file out. Never store credentials in files you submit for assessment.

When a selected path yields nothing to assess, `check` and `export` print a warning to stderr and continue.
Either discovery found no files there, because ignore files or excluded names hide them or the directory is empty,
or no selected contract governs the files found.

Symlinks are not followed. Symbolic links and empty, binary, or non-UTF-8 files are skipped: they stay in the inventory, but no model judges their contents. Unreadable inputs are errors.
There is no configuration file, stored baseline, generated verifier, or automatic fixer.
