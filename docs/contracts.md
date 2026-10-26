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
Numbers have at least three digits, start at 001, and are unique within a scope. `paths` is optional; see
[Scope](#scope). `requires-tenet: X.Y.Z` is optional and names the oldest Tenet that reads the contract; an older
Tenet refuses it. Other frontmatter fields are errors.

Use `Rules` and `Checks` for ordinary contracts. Other headings are welcome; Tenet reads the whole body,
including rationale, exceptions, and ordinary code examples. Keep these inline where possible.

A contract can govern code, configuration, tests, or documentation. State its scope in the body, such as
"Markdown notes under `docs/`". Tenet judges each file the contract governs against it.

Give each contract one concern. Tenet asks one question about the whole contract per file, so a single clause in a
long contract can be missed. Move a mechanical requirement, such as a required metadata field or an import rule, into a
project script and name that script under `Checks`.
`tenet check` withholds legacy tagged evaluation fences from the model.

Commands under `Checks` are instructions for the reviewer. Include the working directory and use project-owned scripts.
Tenet does not run these commands or assume they succeeded. Linked resources are not expanded automatically;
keep required context in the contract or supply repository files with `check --context`.

## Scope

A `.contracts` directory covers files beneath its parent directory.
Root contracts come first, followed by deeper scopes, and contracts within each scope follow numeric order.
Independent checks may run concurrently; results are reported in that order.
Numbers do not override contradictory requirements.

A contract governs every discovered file in its scope except files inside `.contracts` folders. List glob patterns
under `paths` to govern only matching files, such as the source of one language:

```yaml
---
name: python-errors
message: Preserve failures in Python code.
paths: ["**/*.py"]
---
```

Patterns are relative to the directory that holds `.contracts`. `*` matches within one path segment, and `**` matches
any number of directories, so `"*.py"` matches only files directly in that directory. Quote each pattern; YAML reads
a leading `*` as an alias. Files that match no pattern are not assessed and are left out of the contract's inventory.
A pattern that starts with `!` excludes the files it matches, whatever else matches them. A list of only exclusions
governs every file in scope except those. Use one to leave out generated files that are still committed, such as a
lockfile, which ignore files cannot hide from a diff:

```yaml
paths: ["!**/package-lock.json"]
```

A pattern reaches files inside `.contracts` only when it names `.contracts`, such as `".contracts/*/CONTRACT.md"`
for a contract about contracts. `tenet validate` rejects an empty list and absolute, parent-relative, or invalid
patterns, including exclusions. The patterns are part of the document, so editing them leaves earlier conclusions
outdated like any other edit.

```sh
tenet contracts list
tenet contracts list --for src/users.ts
tenet contracts show database-failures
```

`contracts list --for FILE` lists the contracts that govern that file.

## Shared contracts

`tenet contracts add tenet/NAME --scope DIR` installs a contract from the public
[tenet-contracts](https://github.com/macromackie/tenet-contracts) library as an ordinary local file. It takes the next
number in the scope and keeps the library name unless the project already uses it; `--name` chooses another.
`.tenet.lock` records where each copy came from. Put local exceptions under a final `## Local adaptations` heading so
updates merge cleanly. Lines that begin with `<<<<<<< ` or `>>>>>>> ` are unresolved conflict markers and fail
validation. A library contract may also ship tool configuration in `assets/<tool>/`. Such a contract declares
`requires-tenet: 0.11.0`, and `add` copies its assets into the contract's folder as read-only files. See
[Guidance](./guidance.md).

## Examples

Use ordinary fenced code blocks to explain allowed and disallowed behavior. Label them in prose.
Examples are documentation; evaluate real project snapshots and patches through `tenet check`.
See [evaluation](./evaluation.md). Legacy `tenet:` fences are withheld from check inputs to avoid leaking
old expected-answer labels; convert them to ordinary examples when editing a contract. The offline
[export](./export.md) preserves the original document, including those fences.
