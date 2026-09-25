# Commands

```sh
tenet check [PATHS...]
tenet eval
tenet validate
tenet contracts list [--for PATH]
tenet contracts view NAME
```

Paths are relative to the selected project root. The default root is the working directory.
Use `--root DIR` to select another project. Contracts outside that root do not participate.

## Check

```sh
tenet check src/ --jobs 4
tenet check --changed-since origin/main
tenet check src/users.ts --contract database-failures
tenet check --dry-run --json
```

`--changed-since REF` compares the merge base of REF and HEAD with current contents, including staged, unstaged, and untracked files.
It selects files, not introduced violations. Existing problems in changed files may be reported.
Deleted paths are listed separately because they have no current contents.
Contract or ignore changes broaden selection to the project scope to avoid silently missing newly covered files.

Discovery respects Git ignore files and `.ignore`. Hidden files are excluded except `.contracts` during contract discovery.
Generated directories `target`, `node_modules`, `dist`, and `out` are excluded.
Contract documents and their support files are not code subjects.
Binary files are reported as not applicable. Symlinks are not followed; selected symlink files produce errors.

## Options

| Option | Default | Purpose |
| --- | --- | --- |
| `--contract NAME` | all contracts | Select one requirement |
| `--jobs N` | 4 | Maximum simultaneous model requests |
| `--max-requests N` | 1000 | Maximum requests across both passes |
| `--provider NAME` | openrouter | Select provider |
| `--model ID` | provider default | Select a supported pinned model |
| `--reporter KIND` | default | `default`, `verbose`, or `jsonl` |
| `--json` | off | Shorthand for JSONL |

`--dry-run` is available on `check`. It makes no model requests and does not judge relevance.
`validate` and `contracts` also work without credentials.

There is no configuration file, review state, generated verifier, or automatic fixer in this release.
