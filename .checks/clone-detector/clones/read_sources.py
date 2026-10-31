"""Read every declared source, pruning excluded data before opening any file."""

import os
from pathlib import PurePosixPath
from typing import TYPE_CHECKING

from clones.constants import SHEBANG_FORMATS
from clones.errors import CloneRejected
from clones.types import Source

if TYPE_CHECKING:
    from pathlib import Path

    from clones.types import ClonePolicy


def _is_excluded(*, path: Path, policy: ClonePolicy) -> bool:
    relative = PurePosixPath(path.as_posix())
    return any(relative.full_match(pattern) for pattern in policy.exclusions)


def _read_format(*, path: Path, policy: ClonePolicy) -> str | None:
    for name, suffixes in policy.formats:
        if path.suffix in suffixes:
            return name
    if path.suffix:
        return None
    with path.open("rb") as stream:
        header = stream.readline()
    if not header.startswith(b"#!"):
        return None
    for interpreter, name in SHEBANG_FORMATS:
        if interpreter.encode() in header:
            return name
    message = f"read clone source {path.name}: declare its shebang language"
    raise CloneRejected(message)


def _read_source(*, path: Path, root: Path, policy: ClonePolicy) -> Source | None:
    relative = path.relative_to(root)
    if _is_excluded(path=relative, policy=policy) or (
        path.suffix and not any(path.suffix in suffixes for _, suffixes in policy.formats)
    ):
        return None
    if path.is_symlink():
        message = f"read clone source {relative}: symlinks do not establish source ownership"
        raise CloneRejected(message)
    name = _read_format(path=path, policy=policy)
    if name is None:
        return None
    return Source(path=relative, content=path.read_text(encoding="utf-8"), format=name)


def _read_paths(*, root: Path, policy: ClonePolicy) -> set[Path]:
    paths: set[Path] = {root / file for file in policy.files}
    for source_root in policy.roots:
        directory = root / source_root
        if not directory.is_dir() or directory.is_symlink():
            message = f"read clone root {source_root!r}: expected a source directory"
            raise CloneRejected(message)
        for current, directories, files in os.walk(directory):
            parent = root / current
            directories[:] = [
                name
                for name in directories
                if not _is_excluded(path=(parent / name).relative_to(root), policy=policy)
                and not _is_excluded(path=(parent / name / "_").relative_to(root), policy=policy)
            ]
            paths.update(parent / file for file in files)
    return paths


def read_sources(*, root: Path, policy: ClonePolicy) -> tuple[Source, ...]:
    """Deduplicate overlapping roots and include executable extensionless file tasks."""
    paths = _read_paths(root=root, policy=policy)
    sources: list[Source] = []
    for path in sorted(paths):
        source = _read_source(path=path, root=root, policy=policy)
        if source is not None:
            sources.append(source)
    if not sources:
        message = "read clone sources: no declared source file was selected"
        raise CloneRejected(message)
    return tuple(sources)
