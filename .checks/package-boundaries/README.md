# Package dependency checker

Copy `package-boundaries.json` beside the root check configuration and declare every owned package. Each entry names
its manifest, package manager, ownership, and exact internal manifest dependencies. Paths are relative to that root.
Declare copied dependencies, generated output, and fixture input exclusions with a reason. New unlisted manifests
fail the check.

Install the adjacent locked native resolver package once:

```sh
npm --prefix path/to/this/contract/assets/npm ci --ignore-scripts
```

Run the checker from the repository's normal check task:

```sh
uv run --script path/to/this/contract/assets/uv/check-package-boundaries.py \
  --root . --config package-boundaries.json
```

Cargo graphs come from offline, locked `cargo metadata --format-version 1 --no-deps`. Keep the repository's Rust
toolchain available. Local path dependencies retain their physical identity; a registry artifact with the same name
does not become an internal package. Literal Rust module attributes and source inclusions are resolved against their
physical files. Computed source paths and renamed include macros stop the check as incomplete evidence; they are
not treated as forbidden dependency edges.

The locked `dependency-cruiser` and its isolated TypeScript parser resolve JavaScript and TypeScript, including package exports,
configuration aliases, type imports, and dynamic imports. The collector requires every selected source file in the
native report and rejects unresolved imports. The parser stays separate from the application compiler. The `builtins` list declares exact virtual modules provided by the
application runtime; it does not accept wildcard patterns.

`testOnly` is permitted only for a package declared `testSupport`, whose manifest dependency must be development-only.
`testSources` contains patterns relative to the importing package. A production import through that edge fails.
All other package edges count toward acyclicity, including ordinary development dependencies.

## Optional source rules

A consumer governed by a source-module contract can pass `--source-rules path/to/source-rules.json`. That separate
file contains only a standard dependency-cruiser `forbidden` array. The native tool evaluates those rules during the
same resolved graph pass. Source selection and resolution settings remain owned by the collector. Without this
argument, this checker enforces package dependencies and does not impose file-level cycle or import-form rules.

For example, a consumer that requires acyclic source imports can declare:

```json
{
  "forbidden": [
    {
      "name": "acyclic-source-imports",
      "severity": "error",
      "from": {},
      "to": { "circular": true }
    }
  ]
}
```

## Standalone projections

A repository that exports a subtree can derive that subtree's complete policy from the containing declaration.
Pass `--verify-projection path/to/subtree/package-boundaries.json` to require exact projected members, ownership,
edges, and applicable exclusions. Add `--update-projection` when deliberately regenerating that file; ordinary checks
remain read-only. An edge leaving the exported scope cannot be projected as a standalone dependency.

The exported checkout runs this checker against its projected policy without referring to the containing repository.
This keeps one editable dependency authority while preserving independent public checks.
