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

The installed file is yours. Put local exceptions and additions under a final heading so updates merge cleanly:

```markdown
## Local adaptations

Admin export routes may return raw database rows.
```

Tenet reads the whole contract, so the checker sees the rule and its exception together. Edit an upstream sentence only
when your requirement must replace it.

## Updates

```sh
tenet contracts outdated
tenet contracts update
```

`outdated` exits 1 when a contract changed or was removed upstream, or its file is missing. `update` needs committed
contract files and lock. Each copy then takes one path:

| Local copy | Upstream since your release | Result |
| --- | --- | --- |
| Unedited | Changed | Replaced with the new text |
| Edited | Unchanged | Kept; the lock moves to the new release |
| Edited | Changed elsewhere | Merged, keeping both changes |
| Edited | Changed on the same lines | Conflict markers; exit 1 |
| Any | Removed | Left in place; exit 1 |

A renamed copy still counts as unedited: the hash ignores the `name` value. A conflict shows all three versions:

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
update, restore both `.tenet.lock` and the contract files. For a contract removed upstream, follow the changelog's
replacement, then run `tenet contracts remove PATH`, or `--keep-file` to keep it as a local contract.

### Scheduled update pull requests

This workflow opens a pull request when an update is available. It needs no model credentials, and conflicts arrive as
markers that fail `tenet validate` until resolved.

```yaml
# .github/workflows/tenet-contract-updates.yml
name: Tenet contract updates
on:
  schedule:
    - cron: "0 6 * * 1"
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
          curl -fsSL https://tenet-contracts.com/v0.7.0/install.sh | bash
          echo "$HOME/.local/bin" >> "$GITHUB_PATH"
      - id: update
        run: |
          set +e
          tenet contracts outdated; code=$?
          if [ "$code" -eq 0 ]; then exit 0; fi
          if [ "$code" -ne 1 ]; then exit "$code"; fi
          tenet contracts update; code=$?
          if [ "$code" -gt 1 ]; then exit "$code"; fi
          echo "changed=true" >> "$GITHUB_OUTPUT"
      - if: steps.update.outputs.changed == 'true'
        uses: peter-evans/create-pull-request@v7
        with:
          branch: tenet/contract-updates
          title: Update Tenet contracts
          commit-message: Update Tenet contracts
          body: Conflict markers fail `tenet validate` until resolved. Run `tenet contracts diff PATH` for details.
```

### Full checks after an update

A diff check assumes the base followed the old rules, so it cannot judge a changed rule. In pull-request CI, check each
contract whose `CONTRACT.md` changed against the whole repository. This needs provider credentials:

```sh
git fetch origin main
tenet contracts list --base origin/main --json \
  | jq -r '.contract_changes as $c | .contracts[] | select(.path | IN($c[])) | .name' \
  | while read -r name; do tenet check --contract "$name" || exit $?; done
```

## Skills

Install the adoption skill in a project:

```sh
mkdir -p .agents/skills/tenet-adoption
curl -fsSL https://tenet-contracts.com/guidance/skills/tenet-adoption/SKILL.md \
  -o .agents/skills/tenet-adoption/SKILL.md
```

Then ask your agent:

```text
Use tenet-adoption to add code and architecture curation guidance.
Read our existing contracts first. Preserve the package checks and
our rule that only the storage package owns database connections.
```

The agent reads the library at a release tag, chooses relevant files, and edits the project. It records the release
and local choices in `.agents/tenet-adoption.md`. Installed skills work without network access or the library checkout.

| Skill | Use |
| --- | --- |
| `tenet` | Discover contracts and use CLI assessments during review. |
| `tenet-code-curation` | Clarify code and keep implementation patterns consistent. |
| `tenet-architecture-curation` | Review ownership, data flow, recovery, and boundaries. |
| `tenet-test-curation` | Explore during development; retain useful, durable tests. |
| `tenet-contract-curation` | Write and improve readable, scoped contracts. |
| `tenet-adoption` | Adopt skills and contracts, and resolve contract update conflicts. |

Skills describe a workflow; installed contracts define requirements. Keep project commands in repository instructions.
To update skills later:

```text
Use tenet-adoption to review upstream changes since our recorded release.
Keep deliberate local adaptations and explain any changes you decline.
```

Updates arrive as pull requests you review. Nothing updates in the background, and `tenet check` never reads
`.tenet.lock`.
