"""Validate the resolver's complete physical import graph before policy assessment."""

import json
from typing import TYPE_CHECKING

from dependencies.constants import NODE_MODULES_DIRECTORY
from dependencies.errors import DependencyRejected
from dependencies.model import Graph, NativeNodeDependency, NativeNodeModule, ObservedImport
from dependencies.node_issues import read_node_issues

if TYPE_CHECKING:
    from pathlib import Path

    from dependencies.model import NodeProject


def _is_owned(*, root: Path, path: Path, exclusions: tuple[str, ...]) -> bool:
    if not path.is_relative_to(root) or NODE_MODULES_DIRECTORY in path.parts:
        return False
    relative = path.relative_to(root)
    return not any(relative.full_match(pattern) for pattern in exclusions)


def _read_dependency(*, value: object) -> NativeNodeDependency:
    match value:
        case {
            "module": str() as specifier,
            "resolved": str() as resolved,
            "couldNotResolve": bool() as unresolved,
            "coreModule": bool() as builtin,
        }:
            return NativeNodeDependency(
                specifier=specifier,
                resolved=resolved,
                unresolved=unresolved,
                builtin=builtin,
            )
        case _:
            message = "read dependency report: malformed native dependency"
            raise DependencyRejected(message)


def _read_module(*, value: object) -> NativeNodeModule:
    match value:
        case {"source": str() as source, "dependencies": list() as dependencies}:
            return NativeNodeModule(
                source=source,
                dependencies=tuple(_read_dependency(value=item) for item in dependencies),
            )
        case _:
            message = "read dependency report: malformed native module"
            raise DependencyRejected(message)


def _read_modules(*, value: object) -> tuple[NativeNodeModule, ...]:
    match value:
        case {"modules": list() as modules, "summary": {"optionsUsed": {"parser": "tsc"}}}:
            return tuple(_read_module(value=module) for module in modules)
        case _:
            message = "read dependency report: expected the native TypeScript module graph"
            raise DependencyRejected(message)


def _collect_sources(
    *,
    root: Path,
    directory: Path,
    modules: tuple[NativeNodeModule, ...],
    exclusions: tuple[str, ...],
) -> set[str]:
    paths = ((directory / module.source).resolve() for module in modules)
    return {
        path.relative_to(root).as_posix()
        for path in paths
        if _is_owned(root=root, path=path, exclusions=exclusions)
    }


def _read_import(
    *,
    root: Path,
    directory: Path,
    source: str,
    dependency: NativeNodeDependency,
    exclusions: tuple[str, ...],
) -> tuple[ObservedImport, ...]:
    if dependency.unresolved:
        message = f"resolve import {source!r} -> {dependency.specifier!r}: unresolved dependency"
        raise DependencyRejected(message)
    if dependency.builtin:
        return ()
    target = (directory / dependency.resolved).resolve()
    if not _is_owned(root=root, path=target, exclusions=exclusions):
        return ()
    return (
        ObservedImport(
            source_file=source,
            target_file=target.relative_to(root).as_posix(),
            specifier=dependency.specifier,
        ),
    )


def _collect_imports(
    *,
    root: Path,
    directory: Path,
    modules: tuple[NativeNodeModule, ...],
    exclusions: tuple[str, ...],
) -> tuple[ObservedImport, ...]:
    imports: set[ObservedImport] = set()
    for module in modules:
        path = (directory / module.source).resolve()
        if not _is_owned(root=root, path=path, exclusions=exclusions):
            continue
        source = path.relative_to(root).as_posix()
        for dependency in module.dependencies:
            imports.update(
                _read_import(
                    root=root,
                    directory=directory,
                    source=source,
                    dependency=dependency,
                    exclusions=exclusions,
                )
            )
    return tuple(
        sorted(
            imports,
            key=lambda item: (
                item.source_file,
                item.target_file,
                item.specifier,
            ),
        )
    )


def _verify_coverage(*, project: NodeProject, sources: set[str]) -> None:
    missing_sources = set(project.sources) - sources
    if missing_sources:
        message = f"resolve package {project.manifest!r}: skipped {sorted(missing_sources)!r}"
        raise DependencyRejected(message)


def read_node_report(
    *,
    root: Path,
    project: NodeProject,
    content: str,
    exclusions: tuple[str, ...],
) -> Graph:
    """Reject skipped input files so an unavailable parser cannot appear as success."""
    value: object = json.loads(content)
    modules = _read_modules(value=value)
    issues = read_node_issues(value=value)
    directory = (root / project.manifest).parent
    sources = _collect_sources(
        root=root, directory=directory, modules=modules, exclusions=exclusions
    )
    _verify_coverage(project=project, sources=sources)
    return Graph(
        packages=(),
        edges=(),
        issues=issues,
        imports=_collect_imports(
            root=root,
            directory=directory,
            modules=modules,
            exclusions=exclusions,
        ),
    )
