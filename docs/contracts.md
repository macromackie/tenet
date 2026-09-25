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

## Applies to

Code that performs database operations or handles their failures.

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

`name`, `message`, and a nonempty `Rules` section are required. Names are unique throughout the selected project.
Numbers have at least three digits, start at 001, and are unique within a scope. Unknown frontmatter fields are errors.

`Applies to` is optional guidance for relevance. Both decision passes receive the rules and this guidance.
Rationale, examples, expected answers, and other sections do not enter the model prompt.
Write exceptions in `Rules`. Shell commands in prose are not executed.

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
