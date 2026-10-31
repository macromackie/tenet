"""Inspect physical Rust source imports that do not require a Cargo dependency."""

from typing import TYPE_CHECKING

from dependencies.rust import RustSources

if TYPE_CHECKING:
    from pathlib import Path

    from dependencies.model import ObservedImport, ObservedPackage


def read_rust_imports(
    *, root: Path, packages: tuple[ObservedPackage, ...], exclusions: tuple[str, ...]
) -> tuple[ObservedImport, ...]:
    """Resolve literal module paths and source inclusions against their physical source files."""
    return RustSources(root=root, packages=packages, exclusions=exclusions).read()
