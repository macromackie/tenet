# Contracts

Place each contract in a numbered directory:

```text
.contracts/
  001-database-failures/
    CONTRACT.md
```

````markdown
---
name: database-failures
message: Preserve database failures for the caller.
---

## Rules

If a database operation fails, preserve that failure for the caller.
Do not return a successful empty result.

## Checks

From the package directory:

```sh
mise run test
```

## Examples

```typescript tenet:example expect=pass path=src/users.ts name=propagated-error
async function listUsers() {
  return await db.users.list();
}
```

```typescript tenet:example expect=fail path=src/users.ts name=swallowed-error
async function listUsers() {
  try {
    return await db.users.list();
  } catch {
    return [];
  }
}
```
````

`name`, `message`, and a nonempty body are required. Names are unique throughout the project.
Numbers have at least three digits, start at 001, and are unique within a scope. Unknown frontmatter fields are errors.

Use `Rules` and `Checks` for ordinary contracts. Other headings are welcome; Tenet reads the whole body,
including rationale, exceptions, and ordinary code examples. Keep these inline where possible.
Only tagged evaluation fences are withheld from the model, so expected answers cannot leak into checks.

Commands under `Checks` are instructions for the reviewer. Include the working directory and use project-owned scripts.
Tenet does not run these commands or assume they succeeded. Linked resources are not expanded automatically;
keep required context in the contract or supply repository files with `check --context`.

## Scope

A `.contracts` directory covers files beneath its parent directory.
Root contracts run first, followed by deeper scopes. Contracts within each scope run in numeric order.
Each contract finishes before the next starts; file checks within a contract run concurrently.
Numbers do not override contradictory requirements.

```sh
tenet contracts list
tenet contracts list --for src/users.ts
tenet contracts view database-failures
```

## Examples

`tenet:example` fences require a language, `path`, and `expect`. An optional `name` identifies the case in reports.
The path is relative to the contract's scope and cannot escape it. Examples are evaluated independently, without sibling file contents.

Expectations are `pass`, `fail`, `not_applicable`, and `uncertain`.
See [evaluation](./evaluation.md) for interpreting the results.

## Change examples

Use a JSON `tenet:change` fence to evaluate a before/after change under the assumption that the base satisfies the contract:

````markdown
```json tenet:change expect=fail path=src/users.ts name=swallowed-error
{
  "before": "async function users() { return await db.users.list(); }",
  "after": "async function users() { try { return await db.users.list(); } catch { return []; } }"
}
```
````

Use `null` for `before` when adding a file, or for `after` when deleting one. At least one side must have contents.
Expectations remain `pass`, `fail`, `not_applicable`, and `uncertain`; `not_applicable` means the change is unaffected by this requirement.
A regular `tenet:example` fence uses full mode with no baseline assumption.
These examples test individual inputs, not repository-wide coverage.
