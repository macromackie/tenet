# Evidence

`tenet evidence` exports selected source, complete contract documents, and scope information as one JSON object.
It works offline without model credentials. Use it to preserve review inputs or prepare a focused question for another tool.

```sh
tenet evidence src/storage/query.ts --contract database-failures \
  --context src/storage/caller.ts --base origin/main > evidence.json
```

The command accepts the same paths, `--contract`, `--context`, `--base`, `--snapshot`, and `--patch` selection
as [`tenet check`](./cli.md). Output is always JSON; it has no `--json`, provider, or model option.
`--context` supplies supporting source without broadening the requested contract scope.

## Contents

The packet has `version: 1`, `kind: "tenet_evidence"`, and `hash_algorithm: "blake3"`.
Its main fields describe different parts of the review:

| Fields | Meaning |
| --- | --- |
| `mode`, `base`, `head`, `assumption` | Full or diff mode, available Git revisions, and the compliant-base assumption for a diff |
| `selected_paths`, `selected_files`, `support_files` | Requested selectors, selected subjects, and explicit context |
| `inventory`, `omitted_files` | Discovered paths and inventory paths whose source is absent from the packet |
| `contract_changes` | Changed contract paths in a diff |
| `contracts` | Complete documents, identities, scopes, and selection information |
| `sources` | Captured before/after source, keyed by repository-relative path |
| `limits` | `source_bytes`, `capture_bytes`, and `packet_bytes` bounds |

Each contract contains its `name`, `path`, `scope`, complete `document`, and document `hash`. The document preserves
the original Markdown, including any legacy `tenet:` evaluation fences. Unlike model input prepared by `tenet check`,
this archive does not remove those fences or their expected-answer labels.

`rules_span` gives start-inclusive, end-exclusive UTF-8 byte offsets for Rules, or the body if no Rules section exists.
It helps locate text; exceptions and other requirements can appear elsewhere in the contract. Read the complete body.
`requested_scope` is `full_contract` or `selected_subjects`; `selected_files` and `omitted_files` describe that
contract's portion of the packet. These are selection facts, not assessments of compliance or evidence sufficiency.

Each source contains `kind`, `previous_path` for a rename, and `before` and `after` sides.
An available side contains its complete `text` and a BLAKE3 `hash` of those UTF-8 bytes. A missing side is `null`.
Full mode represents current files as additions with no before side. Diff mode preserves each available side,
including the prior source of a deleted file. Supporting files can also appear among the selected subjects;
their source is stored only once.

A source reference needs the path, side, and hash. For an excerpt, also record its line or byte range.
For example, the JSON Pointer `/sources/src~1storage~1query.ts/after` locates the current side of
`src/storage/query.ts`. Escape `/` as `~1` and `~` as `~0` inside each pointer component.
A valid reference establishes which text was used. It does not establish that a description of that text is correct.

Keep the checkout stable during capture. Files are read separately, so the packet is not an atomic filesystem
snapshot. `head` alone does not identify uncommitted contents; use the source hashes. A later `tenet check` captures
its own inputs. Tenet does not import or replay evidence packets.

## Bounds and errors

The default packet limit is 1 MiB. `--max-bytes N` accepts 1 through 67,108,864 bytes (64 MiB) and limits the
complete encoded JSON plus its trailing newline. Each source side remains limited to 64 KiB. Existing
supplementary-context limits also apply; raising the packet limit does not raise them.

Capture has a separate fixed 64 MiB bound, shared by checks, dry runs, and exports. It counts serialized contract
data and original documents, plus each distinct source record including its path, before/after contents, patch, or
read error. Selected and supporting source overlap is counted once. This bounds the captured payload, not total
process memory. Exceeding it aborts before any model request or exported packet; increasing a request budget or
`--max-bytes` does not raise this bound.

Selection follows the normal [discovery rules](./cli.md#discovery). Omitted paths identify discovered source that
was not captured; they are not a dependency analysis. Ignored and otherwise excluded paths may be absent from the
inventory altogether. Choose supporting files explicitly when a question depends on callers or helpers.

Success exits 0 and means the export completed. Invalid, unreadable, binary, or oversized selected inputs exit 2
before any packet is written. Cancellation exits 130. Inputs are never silently truncated.
Unlike a check report, this output contains source text and complete contract documents. Store and share it accordingly.

## Ask a focused question

A packet may be larger than a model tool accepts. For example, ev-grep's `--state` accepts one JSON value up to
64 KiB. Even a one-source packet can exceed that bound after including both sides and contract documents.
Select the evidence needed for one question instead of passing every packet directly.

The following example requires `jq` and an ev-grep version with `--state`. It extracts the complete current source
from the packet above and retains its identity:

```sh
jq -ce --arg path 'src/storage/query.ts' '
  .sources[$path].after as $source
  | if $source == null then error("current source is absent") else
      {hash_algorithm, source: {path: $path, side: "after",
        hash: $source.hash, text: $source.text}}
    end
' evidence.json > question-state.json

ev-grep 'The current source catches a database error and returns an empty result' \
  --state question-state.json --dry-run --json
```

The dry run checks the supplied state without credentials or a model request. Remove `--dry-run` to assess it with
your configured provider; that sends the state to the provider. If it is still too large, choose a smaller question
and explicitly scoped evidence. Retain the original packet and record what the question excludes.

Keep before and after source distinct when asking about a change. A summary can suggest another contract or caller
to inspect, but can lose an exception or misdescribe an interaction. Check the underlying source and the full English
contract before reporting a violation. Neither a favorable answer to one question nor a valid source reference
completes the review.
