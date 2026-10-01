# Guidance

Tenet's optional skills help an agent review code, curate contracts, and improve a codebase.
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

The agent reads the [source snapshot](https://tenet-contracts.com/guidance.json), chooses relevant files,
and edits the project. It records the source revision and local choices in `.agents/tenet-adoption.md`.
Installed skills work without network access or the source checkout.

| Skill | Use |
| --- | --- |
| `tenet` | Discover contracts and use CLI assessments during review. |
| `tenet-code-curation` | Clarify code and keep implementation patterns consistent. |
| `tenet-architecture-curation` | Review ownership, data flow, recovery, and boundaries. |
| `tenet-test-curation` | Explore during development; retain useful, durable tests. |
| `tenet-contract-curation` | Write and improve readable, scoped contracts. |
| `tenet-adoption` | Adopt or update selected guidance while preserving local choices. |

Skills describe a workflow. A project's adopted contracts define its requirements. The collection includes
contract starting points for ownership, data, execution, and contract maintenance; adopting a skill does not
adopt these rules automatically. Keep project commands in repository instructions.

To update later:

```text
Use tenet-adoption to review upstream changes since our recorded revision.
Keep deliberate local adaptations and explain any changes you decline.
```

The agent compares the previous source, current source, and local files when all three are available.
The public endpoint serves the current snapshot. Save it with review evidence if you cannot access the source
repository's Git history. Without the old source, the agent reports that limitation and reviews the two versions.
There is no automatic merge or background update.
