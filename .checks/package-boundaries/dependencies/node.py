"""Combine exact npm manifest membership with resolved source dependency edges."""

from typing import TYPE_CHECKING

from dependencies.model import Graph
from dependencies.read_node_manifests import read_node_manifests
from dependencies.read_source_rules import read_source_rules
from dependencies.resolve_node_sources import resolve_node_sources

if TYPE_CHECKING:
    from pathlib import Path

    from dependencies.model import NodeProject


def read_node_graph(
    *,
    root: Path,
    packages: tuple[NodeProject, ...],
    manifests: tuple[str, ...],
    exclusions: tuple[str, ...],
    source_rules: str = "",
) -> Graph:
    """Use the standard resolver for every source assigned to an owned npm package."""
    manifest_graph = read_node_manifests(root=root, manifests=manifests)
    rules = read_source_rules(root=root, source_rules=source_rules)
    imports = set(manifest_graph.imports)
    issues = list(manifest_graph.issues)
    for project in packages:
        if not project.sources:
            continue
        source_graph = resolve_node_sources(
            root=root,
            project=project,
            exclusions=exclusions,
            rules=rules,
        )
        imports.update(source_graph.imports)
        issues.extend(source_graph.issues)
    return Graph(
        packages=manifest_graph.packages,
        edges=manifest_graph.edges,
        imports=tuple(
            sorted(
                imports,
                key=lambda item: (
                    item.source_file,
                    item.target_file,
                    item.specifier,
                ),
            )
        ),
        issues=tuple(issues),
    )
