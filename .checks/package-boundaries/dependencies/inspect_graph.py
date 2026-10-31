"""Compare exact package ownership with native manifest and import evidence."""

from pathlib import PurePosixPath
from typing import TYPE_CHECKING

from dependencies.constants import NODE_MANAGER
from dependencies.inventory import is_excluded
from dependencies.read_source_owner import read_source_owner

if TYPE_CHECKING:
    from collections.abc import Mapping

    from dependencies.model import (
        AllowedDependency,
        Graph,
        ObservedEdge,
        ObservedImport,
        Package,
        Policy,
    )


def _is_test_source(*, path: str, package: Package) -> bool:
    relative = PurePosixPath(path).relative_to(PurePosixPath(package.manifest).parent)
    return any(relative.full_match(pattern) for pattern in package.test_sources)


def _inspect_manifest_edge(
    *,
    edge: ObservedEdge,
    declarations: Mapping[tuple[str, str], AllowedDependency],
    members: Mapping[str, Package],
) -> tuple[str, ...]:
    if (edge.source, edge.target) not in declarations:
        return ()
    dependency = declarations[edge.source, edge.target]
    errors = []
    if dependency.test_only and not edge.test_only:
        errors.append(
            f"{edge.source}: test-only dependency is in a production manifest section: "
            f"{edge.target}"
        )
    if edge.test_only and members[edge.target].test_support and not dependency.test_only:
        errors.append(
            f"{edge.source}: test-support edge must be explicitly test-only: {edge.target}"
        )
    return tuple(errors)


def _inspect_manifest_edges(*, graph: Graph, policy: Policy) -> tuple[str, ...]:
    declarations = {
        (package.manifest, dependency.target): dependency
        for package in policy.packages
        for dependency in package.dependencies
    }
    observed_edges = {(edge.source, edge.target) for edge in graph.edges}
    members = {package.manifest: package for package in policy.packages}
    errors = [
        f"undeclared internal dependency: {source} -> {target}"
        for source, target in sorted(observed_edges - declarations.keys())
    ]
    errors.extend(
        f"unused declared dependency: {source} -> {target}"
        for source, target in sorted(declarations.keys() - observed_edges)
    )
    errors.extend(
        error
        for edge in graph.edges
        for error in _inspect_manifest_edge(edge=edge, declarations=declarations, members=members)
    )
    return tuple(errors)


def _read_import_packages(
    *, dependency: ObservedImport, packages: tuple[Package, ...], exclusions: tuple[str, ...]
) -> tuple[Package, Package] | tuple[()]:
    if dependency.manager == NODE_MANAGER and any(
        is_excluded(path=path, exclusions=exclusions)
        for path in (dependency.source_file, dependency.target_file)
    ):
        return ()
    members = tuple(package for package in packages if package.manager == dependency.manager)
    source = read_source_owner(path=dependency.source_file, packages=members)
    target = read_source_owner(path=dependency.target_file, packages=members)
    if source is None or target is None:
        return ()
    if source.manifest == target.manifest:
        return ()
    return source, target


def _inspect_import(
    *,
    dependency: ObservedImport,
    packages: tuple[Package, ...],
    declarations: Mapping[tuple[str, str], AllowedDependency],
    exclusions: tuple[str, ...],
) -> tuple[str, ...]:
    owners = _read_import_packages(dependency=dependency, packages=packages, exclusions=exclusions)
    if not owners:
        return ()
    source, target = owners
    edge = (source.manifest, target.manifest)
    if edge not in declarations:
        return (
            (
                f"{dependency.source_file}: import crosses an undeclared package edge to "
                f"{dependency.target_file!r}"
            ),
        )
    if declarations[edge].test_only and not _is_test_source(
        path=dependency.source_file, package=source
    ):
        return (
            (
                f"{dependency.source_file}: production code imports test-support package "
                f"{target.name!r}"
            ),
        )
    return ()


def _inspect_imports(*, graph: Graph, policy: Policy) -> tuple[str, ...]:
    packages = policy.packages
    declarations = {
        (package.manifest, dependency.target): dependency
        for package in policy.packages
        for dependency in package.dependencies
    }
    return tuple(
        error
        for dependency in graph.imports
        for error in _inspect_import(
            dependency=dependency,
            packages=packages,
            declarations=declarations,
            exclusions=policy.exclusions,
        )
    )


def inspect_graph(*, graph: Graph, policy: Policy) -> tuple[str, ...]:
    """Reject missing members, stale declarations, and undeclared resolved package imports."""
    expected_packages = {
        (package.manifest, package.name, package.manager) for package in policy.packages
    }
    actual_packages = {
        (package.manifest, package.name, package.manager) for package in graph.packages
    }
    errors = list(graph.issues)
    errors.extend(
        f"undeclared package: {manifest} ({name}, {manager})"
        for manifest, name, manager in sorted(actual_packages - expected_packages)
    )
    errors.extend(
        f"declared package is missing or renamed: {manifest} ({name}, {manager})"
        for manifest, name, manager in sorted(expected_packages - actual_packages)
    )
    if errors:
        return tuple(errors)
    return (
        *_inspect_manifest_edges(graph=graph, policy=policy),
        *_inspect_imports(graph=graph, policy=policy),
    )
