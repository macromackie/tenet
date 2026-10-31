# Source clone check

Copy `copy-detector.json` into the repository as `.config/copy-detector.json`. Set `sourceRoots`, `sourceFiles`, and
`formats` to cover every first-party programming language, including tests and scripts. Each excluded path needs
a reason. `copy-detector.schema.json` describes the policy.

Run from the repository root, replacing the installed contract path:

```bash
uv run --locked --script .contracts/025-code-duplicated-blocks/assets/uv/check-clones.py --root . --config .config/copy-detector.json
```

The script requires Python 3.14 or later. Its inline metadata and lockfile pin the packaged `jscpd` 5.4.2 executable.
The example policy reports copies of at least 50 tokens and five lines. Declare thresholds before measuring source;
the detector reads files of every length. Extensionless shebang scripts use their interpreter's format.

When an external constraint requires both copies to stay, mark only the copied block with the source language's
comment syntax:

```python
# jscpd:ignore-start -- duplication -- Published migrations must retain their original steps.
apply_published_steps()
# jscpd:ignore-end
```

The adapter rejects missing reasons, malformed pairs, and whole-file markers. It also runs the detector with markers
disabled to reject a marker around a block whose clone no longer exists. The command reports unsuppressed copies
and invalid markers, exits unsuccessfully when either exists, and changes no source files.
