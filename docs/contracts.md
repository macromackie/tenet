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

```typescript
async function listUsers() {
  return await db.users.list();
}
```

```typescript
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

A contract can govern code, configuration, tests, or documentation. State its scope in the body, such as
"Markdown notes under `docs/`", and Tenet judges each file in that scope against the contract.

Give each contract one concern. Tenet asks one question about the whole contract per file, so a single clause in a
long contract can be missed. Move a mechanical requirement, such as a required metadata field or an import rule, into a
project script and name that script under `Checks`.
Legacy tagged evaluation fences are withheld from the model.

Commands under `Checks` are instructions for the reviewer. Include the working directory and use project-owned scripts.
Tenet does not run these commands or assume they succeeded. Linked resources are not expanded automatically;
keep required context in the contract or supply repository files with `check --context`.

## Scope

A `.contracts` directory covers files beneath its parent directory.
Root contracts come first, followed by deeper scopes, and contracts within each scope follow numeric order.
Independent checks may run concurrently; results are reported in that order.
Numbers do not override contradictory requirements.

```sh
tenet contracts list
tenet contracts list --for src/users.ts
tenet contracts view database-failures
```

## Examples

Use ordinary fenced code blocks to explain allowed and disallowed behavior. Label them in prose.
Examples are documentation; evaluate real project snapshots and patches through `tenet check`.
See [evaluation](./evaluation.md). Legacy `tenet:` fences are withheld from model context to avoid leaking
old expected-answer labels; convert them to ordinary examples when editing a contract.
