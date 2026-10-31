"""Resolve a source file to its nearest declared package."""

from pathlib import PurePosixPath
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from dependencies.model import Package


def read_source_owner(*, path: str, packages: tuple[Package, ...]) -> Package | None:
    """Choose the deepest owning manifest without claiming files outside every package."""
    file = PurePosixPath(path)
    owners = tuple(
        package
        for package in packages
        if file.is_relative_to(PurePosixPath(package.manifest).parent)
    )
    if not owners:
        return None
    return max(owners, key=lambda package: len(PurePosixPath(package.manifest).parts))
