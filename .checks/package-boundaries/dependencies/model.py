"""Keep declared ownership separate from native manifest and source observations."""

from dataclasses import dataclass
from typing import TYPE_CHECKING, Literal

if TYPE_CHECKING:
    from collections.abc import Mapping
    from pathlib import Path


@dataclass(frozen=True, kw_only=True)
class ObservedPackage:
    """Identify a package by its repository-relative manifest."""

    name: str
    manifest: str
    manager: Literal["cargo", "npm"]
    entry_files: tuple[str, ...] = ()


@dataclass(frozen=True, kw_only=True)
class ObservedEdge:
    """Retain whether the manifest permits an edge only for tests."""

    source: str
    target: str
    test_only: bool


@dataclass(frozen=True, kw_only=True)
class ObservedImport:
    """Preserve the resolver's physical files and the original import spelling."""

    source_file: str
    target_file: str
    specifier: str
    manager: Literal["cargo", "npm"] = "npm"


@dataclass(frozen=True, kw_only=True)
class Graph:
    """Collect native graphs without treating incomplete resolution as success."""

    packages: tuple[ObservedPackage, ...]
    edges: tuple[ObservedEdge, ...]
    imports: tuple[ObservedImport, ...] = ()
    issues: tuple[str, ...] = ()


@dataclass(frozen=True, kw_only=True)
class AllowedDependency:
    """Name one reviewed internal edge and its permitted use."""

    target: str
    test_only: bool


@dataclass(frozen=True, kw_only=True)
class Package:
    """Declare one package's ownership and exact internal dependencies."""

    name: str
    manifest: str
    manager: Literal["cargo", "npm"]
    ownership: str
    dependencies: tuple[AllowedDependency, ...]
    test_sources: tuple[str, ...]
    test_support: bool
    tsconfig: str
    builtins: tuple[str, ...]


@dataclass(frozen=True, kw_only=True)
class NodeProject:
    """Give the standard resolver each package's configuration and complete source set."""

    manifest: str
    tsconfig: str
    sources: tuple[str, ...]
    builtins: tuple[str, ...] = ()


@dataclass(frozen=True, kw_only=True)
class Policy:
    """Own the package inventory and justified non-source exclusions once."""

    packages: tuple[Package, ...]
    exclusions: tuple[str, ...]


@dataclass(frozen=True, kw_only=True)
class Inventory:
    """Keep all manifests and source files found outside excluded data."""

    cargo: tuple[str, ...]
    node: tuple[str, ...]
    sources: tuple[str, ...]


@dataclass(frozen=True, kw_only=True)
class NodeRegistry:
    """Resolve npm dependency spellings against discovered repository packages."""

    root: Path
    names: Mapping[str, ObservedPackage]
    manifests: Mapping[str, ObservedPackage]


@dataclass(frozen=True, kw_only=True)
class NativeNodeDependency:
    """Validate the physical target and resolution status supplied by the native tool."""

    specifier: str
    resolved: str
    unresolved: bool
    builtin: bool


@dataclass(frozen=True, kw_only=True)
class NativeNodeModule:
    """Keep a native module's complete imports with its source identity."""

    source: str
    dependencies: tuple[NativeNodeDependency, ...]
