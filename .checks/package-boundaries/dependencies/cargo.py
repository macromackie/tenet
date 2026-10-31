"""Read workspace members and internal manifest edges from Cargo's resolved metadata."""

import json
import shutil
import subprocess
from pathlib import Path

from dependencies.errors import DependencyRejected
from dependencies.model import Graph, ObservedEdge, ObservedPackage


def _read_relative_manifest(*, root: Path, value: str) -> str:
    path = Path(value)
    if not path.is_absolute():
        message = f"read Cargo manifest {value!r}: expected Cargo's absolute package path"
        raise DependencyRejected(message)
    path = path.resolve()
    if not path.is_relative_to(root):
        message = (
            f"read Cargo manifest {value!r}: local dependencies must stay inside {str(root)!r}"
        )
        raise DependencyRejected(message)
    return path.relative_to(root).as_posix()


def _resolve_cargo() -> str:
    cargo = shutil.which("cargo")
    if cargo is None:
        message = "read Cargo packages: the pinned Cargo executable is absent from PATH"
        raise DependencyRejected(message)
    return cargo


def _read_package_values(*, root: Path, manifest: str) -> tuple[object, ...]:
    result = subprocess.run(
        [
            _resolve_cargo(),
            "metadata",
            "--format-version",
            "1",
            "--no-deps",
            "--locked",
            "--offline",
            "--manifest-path",
            manifest,
        ],
        cwd=root,
        check=True,
        capture_output=True,
        text=True,
    )
    value: object = json.loads(result.stdout)
    match value:
        case {"version": 1, "packages": [*packages]}:
            return tuple(packages)
        case _:
            message = f"read Cargo metadata {manifest!r}: expected format version 1 packages"
            raise DependencyRejected(message)


def _read_entry_file(*, root: Path, value: object) -> str:
    match value:
        case {"src_path": str() as path}:
            return _read_relative_manifest(root=root, value=path)
        case _:
            message = "read Cargo target: expected its absolute source path"
            raise DependencyRejected(message)


def _read_entry_files(*, root: Path, value: object) -> tuple[str, ...]:
    match value:
        case {"targets": [*targets]}:
            return tuple(sorted({_read_entry_file(root=root, value=target) for target in targets}))
        case _:
            message = "read Cargo package: expected its complete target array"
            raise DependencyRejected(message)


def _read_package(*, root: Path, value: object) -> ObservedPackage:
    match value:
        case {"name": str() as name, "manifest_path": str() as manifest} if name:
            return ObservedPackage(
                name=name,
                manifest=_read_relative_manifest(root=root, value=manifest),
                manager="cargo",
                entry_files=_read_entry_files(root=root, value=value),
            )
        case _:
            message = f"read Cargo package {value!r}: expected a name and manifest path"
            raise DependencyRejected(message)


def _read_dependencies(*, value: object) -> tuple[object, ...]:
    match value:
        case {"dependencies": [*dependencies]}:
            return tuple(dependencies)
        case _:
            message = "read Cargo package: expected its complete dependency array"
            raise DependencyRejected(message)


def _is_test_dependency(*, value: object) -> bool:
    match value:
        case None | "build":
            return False
        case "dev":
            return True
        case _:
            message = f"read Cargo dependency kind {value!r}: expected normal, build, or dev"
            raise DependencyRejected(message)


def _read_dependency_path(*, root: Path, value: object) -> tuple[str, ...]:
    match value:
        case {"path": str() as path}:
            return (_read_relative_manifest(root=root, value=str(Path(path) / "Cargo.toml")),)
        case {"path": None}:
            return ()
        case {"path": _}:
            message = "read Cargo dependency path: expected an absolute package directory"
            raise DependencyRejected(message)
        case _:
            return ()


def _read_edge(*, root: Path, source: str, value: object) -> tuple[ObservedEdge, ...]:
    match value:
        case {"name": str() as name, "kind": kind} if name:
            is_test_only = _is_test_dependency(value=kind)
            targets = _read_dependency_path(root=root, value=value)
            return tuple(
                ObservedEdge(source=source, target=target, test_only=is_test_only)
                for target in targets
            )
        case _:
            message = (
                f"read Cargo dependency {value!r}: expected a package name and dependency kind"
            )
            raise DependencyRejected(message)


def _collect_edges(*, root: Path, values: tuple[object, ...]) -> tuple[ObservedEdge, ...]:
    edges = tuple(
        edge
        for value in values
        for dependency in _read_dependencies(value=value)
        for edge in _read_edge(
            root=root,
            source=_read_package(root=root, value=value).manifest,
            value=dependency,
        )
    )
    edge_test_kinds: dict[tuple[str, str], list[bool]] = {}
    for edge in edges:
        edge_test_kinds.setdefault((edge.source, edge.target), []).append(edge.test_only)
    return tuple(
        ObservedEdge(source=source, target=target, test_only=all(kinds))
        for (source, target), kinds in sorted(edge_test_kinds.items())
    )


def read_cargo_graph(*, root: Path, manifests: tuple[str, ...]) -> Graph:
    """Include every discovered workspace member and every normal, build, and test path edge."""
    root = root.resolve()
    values = tuple(
        value
        for manifest in manifests
        for value in _read_package_values(root=root, manifest=manifest)
    )
    manifest_packages = {
        package.manifest: package
        for package in (_read_package(root=root, value=value) for value in values)
    }
    return Graph(
        packages=tuple(sorted(manifest_packages.values(), key=lambda package: package.manifest)),
        edges=_collect_edges(root=root, values=values),
    )
