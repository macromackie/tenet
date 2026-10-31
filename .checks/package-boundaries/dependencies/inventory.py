"""Discover new packages instead of relying on the already declared member list."""

import os
from pathlib import Path, PurePosixPath
from typing import TYPE_CHECKING

from dependencies.constants import CARGO_MANIFEST, MANIFEST_NAMES, NODE_MANIFEST, SOURCE_SUFFIXES
from dependencies.errors import DependencyRejected
from dependencies.model import Inventory

if TYPE_CHECKING:
    from collections.abc import Iterator


def is_excluded(*, path: str, exclusions: tuple[str, ...]) -> bool:
    """Apply the policy's complete path patterns before reading any source."""
    return any(PurePosixPath(path).full_match(pattern) for pattern in exclusions)


def _walk_files(*, root: Path, exclusions: tuple[str, ...]) -> Iterator[Path]:
    for directory, folders, files in os.walk(root, followlinks=False):
        parent = Path(directory)
        folders[:] = [
            name
            for name in folders
            if not is_excluded(
                path=(parent / name / "__dependency_inventory__").relative_to(root).as_posix(),
                exclusions=exclusions,
            )
        ]
        for name in files:
            yield parent / name


def _is_source(*, root: Path, path: Path, exclusions: tuple[str, ...]) -> bool:
    relative = path.relative_to(root).as_posix()
    if is_excluded(path=relative, exclusions=exclusions):
        return False
    if path.name not in MANIFEST_NAMES and path.suffix not in SOURCE_SUFFIXES:
        return False
    if path.is_symlink():
        message = f"inspect dependency source {relative!r}: source symlinks are not allowed"
        raise DependencyRejected(message)
    return True


def read_inventory(*, root: Path, exclusions: tuple[str, ...]) -> Inventory:
    """Include every manifest and source outside copied, generated, or fixture data."""
    paths = tuple(
        path.relative_to(root)
        for path in _walk_files(root=root, exclusions=exclusions)
        if _is_source(root=root, path=path, exclusions=exclusions)
    )
    return Inventory(
        cargo=tuple(sorted(path.as_posix() for path in paths if path.name == CARGO_MANIFEST)),
        node=tuple(sorted(path.as_posix() for path in paths if path.name == NODE_MANIFEST)),
        sources=tuple(sorted(path.as_posix() for path in paths if path.name not in MANIFEST_NAMES)),
    )
