"""Read exact package identities and internal edges from npm manifests."""

import json
from pathlib import Path
from typing import TYPE_CHECKING

from dependencies.errors import DependencyRejected
from dependencies.model import Graph, NodeRegistry, ObservedEdge, ObservedPackage

if TYPE_CHECKING:
    from collections.abc import Mapping


def _read_package(*, root: Path, manifest: str) -> ObservedPackage:
    value: object = json.loads((root / manifest).read_text(encoding="utf-8"))
    match value:
        case {"name": str() as name} if name.strip():
            return ObservedPackage(name=name, manifest=manifest, manager="npm")
        case _:
            message = f"read npm manifest {manifest!r}: expected a nonempty package name"
            raise DependencyRejected(message)


def _read_path_target(
    *,
    registry: NodeRegistry,
    package: ObservedPackage,
    name: str,
    version: str,
) -> ObservedPackage:
    directory = registry.root / Path(package.manifest).parent / version.partition(":")[2]
    target = (directory / "package.json").resolve()
    if not target.is_relative_to(registry.root):
        message = f"read npm dependency {package.name!r} -> {name!r}: path leaves repository"
        raise DependencyRejected(message)
    relative = target.relative_to(registry.root).as_posix()
    if relative not in registry.manifests:
        message = f"read npm dependency {package.name!r} -> {name!r}: unlisted path {relative!r}"
        raise DependencyRejected(message)
    return registry.manifests[relative]


def _read_target(
    *,
    registry: NodeRegistry,
    package: ObservedPackage,
    name: str,
    version: str,
) -> ObservedPackage | None:
    if version.startswith(("file:", "link:")):
        return _read_path_target(registry=registry, package=package, name=name, version=version)
    alias = name
    if version.startswith("npm:"):
        identifier = version.removeprefix("npm:")
        alias = identifier.partition("@")[0]
        if identifier.startswith("@"):
            alias = "@" + identifier.removeprefix("@").partition("@")[0]
    return registry.names.get(alias)


def _read_versions(
    *,
    value: object,
    package: ObservedPackage,
    field: str,
) -> Mapping[str, str]:
    if not isinstance(value, dict):
        message = f"read npm manifest {package.manifest!r}: expected {field!r} to be an object"
        raise DependencyRejected(message)
    versions: dict[str, str] = {}
    for name, version in value.items():
        if not isinstance(name, str) or not isinstance(version, str):
            message = f"read npm dependency {package.manifest!r}: expected name and version strings"
            raise DependencyRejected(message)
        versions[name] = version
    return versions


def _read_section_value(*, root: Path, package: ObservedPackage, field: str) -> Mapping[str, str]:
    value: object = json.loads((root / package.manifest).read_text(encoding="utf-8"))
    match value:
        case dict() as document:
            section: object = document.get(field, {})
        case _:
            message = f"read npm manifest {package.manifest!r}: expected an object"
            raise DependencyRejected(message)
    return _read_versions(value=section, package=package, field=field)


def _read_section(
    *,
    registry: NodeRegistry,
    package: ObservedPackage,
    field: str,
    test_only: bool,
) -> tuple[ObservedEdge, ...]:
    section = _read_section_value(root=registry.root, package=package, field=field)
    targets = tuple(
        _read_target(registry=registry, package=package, name=name, version=version)
        for name, version in section.items()
    )
    return tuple(
        ObservedEdge(source=package.manifest, target=target.manifest, test_only=test_only)
        for target in targets
        if target is not None
    )


def _create_registry(*, root: Path, packages: tuple[ObservedPackage, ...]) -> NodeRegistry:
    names: dict[str, ObservedPackage] = {}
    for package in packages:
        if package.name in names:
            message = f"read npm membership: duplicate package name {package.name!r}"
            raise DependencyRejected(message)
        names[package.name] = package
    return NodeRegistry(
        root=root,
        names=names,
        manifests={package.manifest: package for package in packages},
    )


def _collect_edges(
    *, registry: NodeRegistry, packages: tuple[ObservedPackage, ...]
) -> tuple[ObservedEdge, ...]:
    edges: dict[tuple[str, str], bool] = {}
    for package in packages:
        for field, test_only in (
            ("dependencies", False),
            ("devDependencies", True),
            ("peerDependencies", False),
            ("optionalDependencies", False),
        ):
            for edge in _read_section(
                registry=registry,
                package=package,
                field=field,
                test_only=test_only,
            ):
                identity = (edge.source, edge.target)
                edges[identity] = edges.get(identity, True) and edge.test_only
    return tuple(
        ObservedEdge(source=source, target=target, test_only=test_only)
        for (source, target), test_only in sorted(edges.items())
    )


def read_node_manifests(*, root: Path, manifests: tuple[str, ...]) -> Graph:
    """Discover membership independently from the policy's allowed package list."""
    packages = tuple(_read_package(root=root, manifest=manifest) for manifest in manifests)
    registry = _create_registry(root=root, packages=packages)
    return Graph(packages=packages, edges=_collect_edges(registry=registry, packages=packages))
