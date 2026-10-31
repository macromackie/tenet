"""Read native package graphs and enforce their single ownership declaration."""

from typing import TYPE_CHECKING

from dependencies.cargo import read_cargo_graph
from dependencies.constants import NODE_MANAGER
from dependencies.errors import DependencyRejected
from dependencies.inspect_graph import inspect_graph
from dependencies.inventory import read_inventory
from dependencies.model import Graph, NodeProject
from dependencies.node import read_node_graph
from dependencies.read_source_owner import read_source_owner
from dependencies.rust_sources import read_rust_imports
from dependencies.validate_policy import validate_policy

if TYPE_CHECKING:
    from pathlib import Path

    from dependencies.model import Policy


def _read_projects(*, policy: Policy, sources: tuple[str, ...]) -> tuple[NodeProject, ...]:
    packages = tuple(package for package in policy.packages if package.manager == NODE_MANAGER)
    manifest_sources: dict[str, list[str]] = {package.manifest: [] for package in packages}
    for source in sources:
        owner = read_source_owner(path=source, packages=packages)
        if owner is None:
            message = f"inspect source {source!r}: no package owns this source"
            raise DependencyRejected(message)
        manifest_sources[owner.manifest].append(source)
    return tuple(
        NodeProject(
            manifest=package.manifest,
            tsconfig=package.tsconfig,
            sources=tuple(manifest_sources[package.manifest]),
            builtins=package.builtins,
        )
        for package in packages
    )


def inspect_dependencies(*, root: Path, policy: Policy, source_rules: str = "") -> tuple[str, ...]:
    """Fail on incomplete resolution as well as undeclared or obsolete internal edges."""
    errors = validate_policy(policy=policy)
    if errors:
        return errors
    inventory = read_inventory(root=root, exclusions=policy.exclusions)
    cargo = read_cargo_graph(root=root, manifests=inventory.cargo)
    rust_imports = read_rust_imports(
        root=root, packages=cargo.packages, exclusions=policy.exclusions
    )
    node = read_node_graph(
        root=root,
        packages=_read_projects(policy=policy, sources=inventory.sources),
        manifests=inventory.node,
        exclusions=policy.exclusions,
        source_rules=source_rules,
    )
    graph = Graph(
        packages=(*cargo.packages, *node.packages),
        edges=(*cargo.edges, *node.edges),
        imports=(*rust_imports, *node.imports),
        issues=(*cargo.issues, *node.issues),
    )
    return inspect_graph(graph=graph, policy=policy)
