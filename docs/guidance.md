# Guidance

The public [tenet-contracts](https://github.com/macromackie/tenet-contracts) library holds shared contracts and agent
skills. A contract becomes a requirement only when a project installs it. Releases are `vX.Y.Z` tags, and the
[changelog](https://github.com/macromackie/tenet-contracts/blob/main/CHANGELOG.md) marks each tightened, loosened,
clarified, added, or removed rule. A tightened or removed rule is a major release.

Contract names start with an area, followed by any technology the rules depend on. `security-api-key-lifecycle`
applies to any stack; `ui-react-interaction-ownership` applies only to React code. The library
[README](https://github.com/macromackie/tenet-contracts#contracts) lists every contract by area.

## Shared contracts

Install a contract into the scope it should govern:

```sh
tenet contracts add tenet/data-and-apis --scope packages/api
tenet check --contract data-and-apis
```

```text
added packages/api/.contracts/003-data-and-apis/CONTRACT.md from tenet/data-and-apis v1.0.0
next: tenet check --contract data-and-apis
```

`add` takes the highest release unless you name one, as in `tenet/data-and-apis@v1.0.0`. It writes the next numbered
folder and records the source, release, commit, and content hash in `.tenet.lock` at the project root. Commit both.
A name already used in the project needs `--name`, such as `--name api-data-and-apis`. Check a new contract without
`--base`, because no earlier commit was reviewed against it.

A contract may ship tool configuration, such as an oxlint rules file, in an `assets/` folder. `add` copies it beside
the installed `CONTRACT.md` and records each file's hash in the lock. Point your tool's configuration at those files
and keep your own settings in a file of your own: installed assets are read-only, and `tenet validate` fails while one
is edited or missing. Exclude `.contracts/**` and installed skill folders from formatters, because a reformatted asset
or script no longer matches its hash. `add` refuses an asset that Git ignores and names the ignore rule, because Git
would never commit it. A contract with assets declares `requires-tenet: 0.11.0`, and older Tenet versions refuse it.

The installed `CONTRACT.md` is yours. Put local exceptions and additions under a final heading so updates merge
cleanly:

```markdown
## Local adaptations

Admin export routes may return raw database rows.
```

Tenet reads the whole contract, so the checker sees the rule and its exception together. Edit an upstream sentence only
when your requirement must replace it. Files you add to the contract folder are not in the lock, and Tenet never
touches them.

## Updates

```sh
tenet contracts outdated
tenet contracts update
```

`outdated` exits 1 when a contract or one of its assets changed upstream, a contract was removed, or an installed file
is missing or an asset edited. It also names the major releases a copy crosses:

```text
packages/api/.contracts/003-data-and-apis/CONTRACT.md  data-and-apis  tenet/data-and-apis  v9.2.0 -> v12.0.0 (crosses 3 majors; update stops at v10.1.0)  unchanged
```

`update` stops at the next major. Here it moves to `v10.1.0`, the highest `v10` release, so each major arrives as its
own reviewed change; read its changelog entry before you merge it. `--ref v12.0.0` moves past it at once. `--ref` with
an earlier release rolls an update back and merges local edits onto the older text.

`update` needs committed contract files and lock. Each copy then takes one path:

| Local copy | Upstream since your release | Result                                  |
| ---------- | --------------------------- | --------------------------------------- |
| Unedited   | Changed                     | Replaced with the new text              |
| Edited     | Unchanged                   | Kept; the lock moves to the new release |
| Edited     | Changed elsewhere           | Merged, keeping both changes            |
| Edited     | Changed on the same lines   | Conflict markers; exit 1                |
| Any        | Removed                     | Left in place; exit 1                   |

A renamed copy still counts as unedited: the hash ignores the `name` value. Hashes of assets and skill files read CRLF
line endings as LF, so Git's line-ending settings never fail `tenet validate`. A contract's own hash covers its exact
bytes, as in Tenet 0.10.0, so a checkout that converts its line endings shows it as edited and merges it.

Assets are never merged. `update` replaces an unedited asset and writes one that is new upstream. While a selected
contract has an edited or missing asset, it refuses before writing anything; `tenet contracts diff PATH` shows how the
asset differs. An asset removed upstream stays in place as your own file, leaves the lock, and makes `update` exit 1.

A conflict shows all three versions:

```text
<<<<<<< local
Validate untrusted inputs at their entry point, except on admin routes.
||||||| base v1.0.0
Validate untrusted inputs at their entry point.
=======
Validate untrusted inputs at their entry point and reject them there.
>>>>>>> upstream v1.1.0
```

`tenet validate` fails while markers remain. Resolve them by hand or with the `tenet-adoption` skill, then run a full
check of each changed contract. `tenet contracts diff PATH` compares a copy with its recorded release. To abandon an
update, restore both `.tenet.lock` and the installed files. For a contract removed upstream, follow the changelog's
replacement, then run `tenet contracts remove PATH`, or `--keep-file` to keep it as a local contract.

### Scheduled update pull requests

This workflow opens a pull request when an update is available. It needs no model credentials, and conflicts arrive as
markers that fail `tenet validate` until resolved. `update` stops at the next major, so each major gets its own pull
request on a branch named for it, such as `tenet/contract-updates-v10`. Later runs refresh that pull request until you
merge it; the run after the merge opens the next major.

```yaml
# .github/workflows/tenet-contract-updates.yml
name: Tenet contract updates
on:
  schedule:
    - cron: '0 6 * * 1'
  workflow_dispatch:
permissions:
  contents: write
  pull-requests: write
jobs:
  update:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - run: |
          curl -fsSL https://tenet-contracts.com/v0.12.1/install.sh | bash
          echo "$HOME/.local/bin" >> "$GITHUB_PATH"
      - id: update
        run: |
          set +e
          tenet contracts outdated; code=$?
          if [ "$code" -eq 0 ]; then exit 0; fi
          if [ "$code" -ne 1 ]; then exit "$code"; fi
          tenet contracts update --json > "$RUNNER_TEMP/update.json"; code=$?
          if [ "$code" -gt 1 ]; then exit "$code"; fi
          majors=$(jq -r '[.contracts[].to | (capture("^(?<major>v[0-9]+)[.]").major // .)] | unique | join("-")' \
            "$RUNNER_TEMP/update.json")
          echo "majors=$majors" >> "$GITHUB_OUTPUT"
      - if: steps.update.outputs.majors != ''
        uses: peter-evans/create-pull-request@v7
        with:
          branch: tenet/contract-updates-${{ steps.update.outputs.majors }}
          title: Update Tenet contracts to ${{ steps.update.outputs.majors }}
          commit-message: Update Tenet contracts to ${{ steps.update.outputs.majors }}
          body: Conflict markers fail `tenet validate` until resolved. Run `tenet contracts diff PATH` for details.
```

### Full checks after an update

A diff check assumes the base followed the old rules, so it cannot judge a changed rule. In pull-request CI, check each
contract whose folder changed against the whole repository, including a contract whose update changed only an asset.
This needs provider credentials:

```sh
git fetch --unshallow origin main 2>/dev/null || git fetch origin main
tenet contracts list --base "$(git merge-base origin/main HEAD)" --json \
  | jq -r '.contract_changes as $c | .contracts[] | select(.path | IN($c[])) | .name' \
  | { code=0; while read -r name; do tenet check --contract "$name" || code=$?; done; exit "$code"; }
```

Comparing with the merge base leaves out contracts that changed only on `main`. The job checks every changed contract
and exits with the last non-zero code. Exit 4 means a model answer awaits
review. Records stay in the `.tenet/` folder of the machine that wrote them, and Tenet exits 2 if Git tracks that
folder, so CI never sees them and reports the contract on every run.

## Skills

Install a library skill with `tenet skills add`, which needs Tenet 0.11.0 or later:

```sh
tenet skills add tenet/tenet-adoption
```

```text
added .agents/skills/tenet-adoption from tenet/tenet-adoption v1.0.0
next: tenet validate
```

`skills add` copies the skill's whole folder, such as its `SKILL.md`, `references/`, and `LICENSE.txt`, into
`.agents/skills/NAME` and records each file's hash in `.tenet.lock`. Commit both. `--dir DIR` installs into another
directory, and `--name LOCAL` renames the folder and the skill's `name`. Tenet refuses a destination that already exists
or that Git ignores, and names the ignore rule. It never writes provenance into `SKILL.md`; the lock records it.

Codex, GitHub Copilot, Cursor, Gemini CLI, and OpenCode read `.agents/skills`. Claude Code reads only
`.claude/skills/`, so commit a relative symlink to each installed skill:

```sh
mkdir -p .claude/skills
ln -s ../../.agents/skills/tenet-adoption .claude/skills/tenet-adoption
```

Tenet ignores the symlink, so name it after the local name when you install with `--name`, and delete it when you run
`tenet skills remove`. Claude Code project cloud threads load no repository skills, only the skills enabled on the
account, so the symlink does not reach them. There, enable the skill on the account, or name its file in the request,
as in `Read .agents/skills/tenet-adoption/SKILL.md`.

Then ask your agent:

```text
Use tenet-adoption to add code and architecture curation guidance.
Read our existing contracts first. Preserve the package checks and
our rule that only the storage package owns database connections.
```

The agent reads the library at a release tag, chooses relevant skills and contracts, and installs them with
`tenet skills add` and `tenet contracts add`. It records only its decisions in `.agents/tenet-adoption.md`, because the
lock records releases and hashes. Installed skills work without network access or the library checkout.

| Skill                         | Use                                                                |
| ----------------------------- | ------------------------------------------------------------------ |
| `tenet`                       | Discover contracts and use CLI assessments during review.          |
| `tenet-code-curation`         | Clarify code and keep implementation patterns consistent.          |
| `tenet-architecture-curation` | Review ownership, data flow, recovery, and boundaries.             |
| `tenet-test-curation`         | Explore during development; retain useful, durable tests.          |
| `tenet-contract-curation`     | Write and improve readable, scoped contracts.                      |
| `tenet-adoption`              | Adopt skills and contracts, and resolve contract update conflicts. |

Skills describe a workflow; installed contracts define requirements. Keep project commands in repository instructions.

### Skill updates

```sh
tenet skills outdated
tenet skills update
```

These work like the contract commands above. `SKILL.md` and the skill's other Markdown files merge three ways and keep
your edits; conflicts arrive as markers that fail `tenet validate`. Other files, such as scripts, are read-only, as
contract assets are. `tenet validate` also checks that each installed `SKILL.md` has a `name` equal to its folder and a
`description`. It keeps every other frontmatter key, so Claude Code fields such as `paths` keep working. Contract and
skill updates share the lock, so commit one update before starting the other.

Updates arrive as pull requests you review. Nothing updates in the background, and `tenet check` never reads
`.tenet.lock`.
