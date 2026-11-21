# JSON

Every JSON object Tenet prints carries a `version`. JSON goes to stdout, and with JSON requested, progress and
warnings go to stderr. Each format numbers its versions independently:

| Command                                                     | Output                                                     | Version |
| ----------------------------------------------------------- | ---------------------------------------------------------- | ------- |
| `init --json`                                               | One onboarding recommendation or scaffold result           | 1       |
| `doctor --json`                                             | One environment and repository readiness report            | 1       |
| `check --json`, `check --reporter jsonl`                    | One [event](./output.md#jsonl) per line                    | 5       |
| `check --dry-run --json`                                    | One event per line: `selected`, `combined`, then `summary` | 5       |
| `check --qualify CONTRACT/NAME --json`                      | One checker qualification report                           | 1       |
| `check --final --json`                                      | One [final check report](./final-checks.md)                | 1       |
| `status --json`                                             | One object                                                 | 1       |
| `show NAME --json`, `show --pending --json`                 | One object                                                 | 2       |
| `record NAME ... --json`                                    | One object with `record`, `replaces`, and `exit_code`      | 1       |
| `log --json`                                                | One review log entry per line                              | 2       |
| `export`                                                    | One [evidence packet](./export.md#contents)                | 2       |
| `contracts list --json`                                     | One object                                                 | 1       |
| `contracts outdated --json`, `update --json`, `diff --json` | One object                                                 | 1       |
| `skills outdated --json`, `update --json`, `diff --json`    | One object                                                 | 1       |

## Compatibility

A format's version changes when a documented field is removed or renamed, when its type changes, or when an existing
value starts to mean something else. A release can do the following without a new version:

- Add a field to any object or event. A list that is usually empty, such as a scoped file's `changed` or `excerpt`
  lines, can be left out when empty; read a missing list as empty.
- Add a `check` event type.
- Add a value to a field that names a kind: a conclusion, a review `state` or pending `reason`, a `reason_code`, a
  failure `kind`, a file `status`, an `omitted` part, a log entry `type`, or an adoption `state`, `change`, or
  `outcome`.

Ignore fields and event types you do not know. An unknown kind means the result cannot be interpreted, not that it
passed: treat an unknown conclusion or pending reason as pending, and an unknown file status as needing review. Decide
the outcome from `exit_code` or the process exit code, which keep the meanings in [Output](./output.md#check-exit-codes).

Versions are not aligned across formats. Raising every format to one number would change versions that consumers of
`check` events and `status` compare against today without changing a field.

## Errors

When `init`, `doctor`, `check`, `fix`, `status`, `show`, `record`, or `log` fails with JSON output requested, stdout ends with one `check` error event,
stderr repeats its message, and the command exits 2. A `check` that stops partway, such as on rejected credentials,
prints its earlier events first:

```json
{
  "version": 5,
  "type": "error",
  "message": "logging is not part of the current review; see tenet status",
  "exit_code": 2
}
```

The event keeps `check`'s version 5 for every command. Look for `"type": "error"` before reading a `status` or `show`
object, which has no `type`. Other commands report errors on stderr only.

## Local checks

Each row in `checks` names the `contract` and `check`. Read `freshness` alongside `observed_outcome`. A stale pass describes an earlier observation. `message` holds the diagnostic and `replay` gives the command to run from the project root. A structured checker can also return a `finding` with its case, expected and actual values, and location. A reused result has `reused: true`. See [Local checks](./local-checks.md) for binding and exit semantics.

## Check

[Output](./output.md#jsonl) describes `check` events. `check --dry-run --json` makes no model request and prints:

- `selected` for each contract and file: `contract`, `scope`, `path`, a renamed file's `previous_path`,
  `contract_hash`, `input_bytes`, `input_limit_bytes`, `omitted` parts, and a `status` and `reason` for a file that
  would be skipped, incomplete, or an error. An oversized individual diff also lists `hunks`, with each
  planned group's `ranges` and encoded `input_bytes`.
- `combined` for each contract: its repository `context`, or an `error`.
- `summary` with `dry_run: true`, `files`, `mode`, `assume_base_valid`, `deleted`, `max_requests`,
  `combined_over_limit`, `errors`, the diff's [`coverage`](./output.md#coverage), and `exit_code`.
  `requests_to_send` counts planned new dispatches, split into `file_requests_to_send` and
  `combined_requests_to_send`. `cached` counts reused answer entries, not provider calls.
  The existing `requests` planning total includes cached entries for compatibility; use `requests_to_send` for new work.
  `file_pairs` counts selected file/contract checks, including skipped or incomplete checks, and `cached_file_pairs`
  counts ordinary file-pair answers reused from the cache. Cached hunk groups contribute to `cached` separately.
  `requests_by_contract` retains per-contract planning counts, including cached answers and hunk groups; a shared
  dispatch is not a separate charge for each contract it serves. The summary also records `context_profile`,
  `input_limit_bytes`, and `contract_batch_size`.

Live conclusions and saved `show` conclusions include `combined_status`, independently of the contract's `reason_code`.
Its values are `answered`, `too_large`, `request_budget`, `input_unavailable`, `request_failed`, and `not_needed`.
Saved conclusions retain `context`, `evidence_hash`, and `failure` when available.
Saved file results retain their `reason` and `failure`, so reopening a review explains input limits and failed requests.
An absent field in an older log means that fact was not recorded.

The `begin` event records the selected `context_profile` and `contract_batch_size`. File and hunk results retain the
actual `input_limit_bytes` beside their measured `input_bytes`; a combined context retains its own `limit_bytes`.
Do not infer a limit from the CLI default when reading a saved run.

Contract grouping emits a `file_batch_started` event with `request_key`, `path`, `contracts`, `questions`, and
`input_bytes`, followed by `file_batch_completed` with the same key and its provider `request` receipt or `failure`.
Member file results carry `shared_request`, identifying the original shared request, without another copy of its
usage receipt. Saved file results preserve that provenance. Count usage once per completed dispatch, not once per
member result or question. See [request accounting](./output.md#request-accounting).

An unchanged logical pair can be reused after other contracts join or leave a request group. Its original
`shared_request` remains provenance; it does not mean the old group was dispatched again. Switching context profiles
or between isolated and grouped question protocols requires new answers. `--fresh` bypasses existing answers.

An oversized diff's file result retains `hunks` in live and saved output.
Each group identifies supplied `ranges` with inclusive `old` and `new` line pairs, using `null` for an empty side.
The group retains `input_bytes`, its encoded-input `evidence_hash` when prepared for assessment, its routed `status`, raw `applicability` and
`verification` choices and confidence, the violation probability, and any `reason` or `failure`.
These are advisory fragment results. The enclosing file remains incomplete or error.
The live `hunk_summary` counts group outcomes and request activity separately from the run's file counts.
Each group emits a `hunk_result` event with its contract, path, result, `cached` flag, and provider `request` receipt as soon as it completes,
so completed evidence remains in JSONL even if a later group stops the run.
The receipt is absent for cached answers and unsent or failed requests. Count a group's cost once from its event;
the enclosing file repeats its evidence without another request receipt. Older hunk events lack this metadata.
The run's `execution_errors` counts failures outside provider requests, including failures inside hunk checks.

A `context` lists `excerpts` when [`--context`](./reviews.md#add-context) supplied only some lines of a file: each
path maps to its supplied inclusive line ranges, as in `{"src/storage/query.ts": [[40, 75]]}`. A selected file is
listed when its lines took the place of contents left out to fit.

## Status

`status --json` prints the `review` label, the `base` commit, the latest `revision`, `unchanged` when the working tree
matches the base, `contracts`, `deleted_contracts`, and `exit_code`. Each contract has:

| Field                                      | Meaning                                                                                                                  |
| ------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ |
| `state`                                    | `settled`, with `conclusion` and the deciding `record` (`null` when the model settled it), or `pending`, with a `reason` |
| `contract`, `path`                         | The contract's name and document path                                                                                    |
| `selection`                                | The check whose input this row reports: its revision `rev` and selected `paths`                                          |
| `input`                                    | The hash of the contract's current input                                                                                 |
| `assessed_in`                              | The revision whose model assessment had this input, or `null`                                                            |
| `model`, `model_violation`, `model_answer` | That assessment's conclusion, violation probability, and raw answer                                                      |
| `min_confidence`                           | The threshold that check used                                                                                            |

When no check compares the working tree with the base, the object has `revision: null`, empty `contracts`,
`exit_code: 4`, and a `message`.

## Show

`show NAME --json` prints one contract:

| Field                                                            | Meaning                                                                                                                                                                                      |
| ---------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `contract`, `document`                                           | Name, message, path, scope, and hash, and the complete document                                                                                                                              |
| `state`, `selection`, `mode`, `scope`, `input`, `min_confidence` | As in `status`, plus the check's mode and requested scope                                                                                                                                    |
| `assessment`                                                     | The latest model assessment of this input: `rev`, `input`, `scope`, `contract_hash`, `conclusion`, `carried_from`, and file `results`                                                        |
| `files`                                                          | Every scoped file: `role`, `path`, `previous_path`, `before` and `after` content hashes, `error`, the `changed` lines in a diff, the `excerpt` lines named with `--context`, and its `patch` |
| `records`, `earlier`, `older`                                    | The latest five records and earlier assessments, and counts of the older ones                                                                                                                |
| `changes_since_earlier`                                          | What changed since the latest assessment of a different input                                                                                                                                |
| `conclusions`                                                    | The conclusions `record` accepts for this mode and scope                                                                                                                                     |

`show --pending --json` prints `contracts`, each pending contract in that form without `version` or file patches,
and `changes`, the `path` and `patch` of each changed file once.

## Log

`log --json` prints each entry of `.tenet/events.jsonl`, oldest first, with `version: 2` and a `type` of
`revision`, `assessment`, or `record`. A revision's `context` repeats the `--context` arguments, including any line
range, as in `src/storage/query.ts:40-75`.
Its `context_profile` records the input bounds used to capture and assess the revision, and `contract_batch_size`
records the request grouping. Missing settings in an older revision mean `standard` and batch size 1. Reopening the
review and printing follow-up commands preserves its recorded settings.

## Contracts

| Command                      | Fields                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 |
| ---------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `contracts list --json`      | `contracts`, each with `name`, `path`, `scope`, and `message`; `contract_changes`, the changed contract paths of a diff; and the diff's [`coverage`](./output.md#coverage), `null` without `--base`                                                                                                                                                                                                                                                                                                                    |
| `contracts outdated --json`  | `contracts`, each with `path`, local `name`, `source`, `contract`, recorded `ref`, `state` of its `CONTRACT.md` (`unchanged`, `adapted`, `edited`, or `missing`), `files`, the `state` of each edited or missing asset, and `newest` (`ref`; `change` of the `CONTRACT.md`: `current`, `changed`, or `removed`; `files`, the `change` of each asset upstream changed, `added`, or `removed`; and `majors`, the highest release of each major crossed, oldest first, where `update` stops at the first; `null` offline) |
| `contracts update --json`    | `contracts`, each with `path`, `from`, `to`, the `outcome` of its `CONTRACT.md`: `unchanged`, `replaced`, `merged`, `conflict` with `regions`, or `removed`, and `files`, an object with the `outcome` of each asset that did not stay unchanged: `replaced`, `added`, or `removed`                                                                                                                                                                                                                                    |
| `contracts diff PATH --json` | `contract`, with `path`, the recorded `base` and the `upstream` release (each `ref`, `commit`, and `text`; `upstream` is `null` when removed), the `local` text, and `files`, each asset that differs, with its `path` and its `base`, `upstream`, and `local` text, each `null` when that version lacks it                                                                                                                                                                                                            |

`skills outdated --json` and `skills update --json` print the same objects under `skills`, and `skills diff PATH
--json` under `skill`, with `skill` in place of `contract`. A skill's `path` is its installed folder, and its `files`
cover every file but `SKILL.md`; an edited Markdown file can also be `merged` or a `conflict`. `files` paths are relative to the copy's
folder, and `files` is left out when empty. `diff` reads a file that is not UTF-8 with replacement characters.
