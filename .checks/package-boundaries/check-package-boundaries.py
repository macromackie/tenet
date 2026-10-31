#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.14"
# dependencies = ["tree-sitter==0.25.2", "tree-sitter-rust==0.24.2"]
# ///
"""Check exact declared package members, internal dependencies, and test-only boundaries."""

from pathlib import Path

from dependencies import inspect_dependencies, read_policy
from dependencies.arguments import read_arguments
from dependencies.errors import DependencyRejected
from dependencies.projection import inspect_projection


def main() -> None:
    """Read one ownership policy and report every native dependency disagreement."""
    arguments = read_arguments()
    root = Path(arguments.root).resolve()
    policy = read_policy(path=root / arguments.config)
    errors = (
        *inspect_projection(
            root=root,
            source=arguments.config,
            destination=arguments.verify_projection,
            policy=policy,
            update=arguments.update_projection,
        ),
        *inspect_dependencies(root=root, policy=policy, source_rules=arguments.source_rules),
    )
    if errors:
        raise DependencyRejected("\n".join(errors))


if __name__ == "__main__":
    main()
