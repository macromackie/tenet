"""Decode a closed clone policy without accepting detector-narrowing options."""

import json
from pathlib import PurePosixPath
from typing import TYPE_CHECKING

from clones.constants import NATIVE_SUFFIXES, PARENT_COMPONENT
from clones.errors import CloneRejected
from clones.types import ClonePolicy

if TYPE_CHECKING:
    from pathlib import Path


def _read_path(*, value: object) -> str:
    if not isinstance(value, str) or not value.strip():
        message = f"read clone path {value!r}: expected a nonempty relative path"
        raise CloneRejected(message)
    path = PurePosixPath(value)
    if path.is_absolute() or PARENT_COMPONENT in path.parts:
        message = f"read clone path {value!r}: expected a path inside the repository"
        raise CloneRejected(message)
    return value


def _read_bound(*, value: object) -> int:
    if not isinstance(value, int) or isinstance(value, bool) or value < 1:
        message = f"read clone bound {value!r}: expected a positive integer"
        raise CloneRejected(message)
    return value


def _read_exclusion(*, value: object) -> str:
    match value:
        case {"pattern": str() as pattern, "reason": str() as reason, **unknown} if (
            reason.strip() and not unknown
        ):
            return _read_path(value=pattern)
        case _:
            message = f"read clone exclusion {value!r}: declare a pattern and its reason"
            raise CloneRejected(message)


def _read_suffix(*, value: object) -> str:
    if (
        not isinstance(value, str)
        or not value.startswith(".")
        or PurePosixPath("source" + value).suffix != value
    ):
        message = f"read clone suffix {value!r}: expected one file extension"
        raise CloneRejected(message)
    return value


def _read_format(*, name: object, suffixes: object) -> tuple[str, tuple[str, ...]]:
    if not isinstance(name, str) or name not in NATIVE_SUFFIXES:
        message = f"read clone format {name!r}: expected a supported language"
        raise CloneRejected(message)
    if not isinstance(suffixes, list) or not suffixes:
        message = f"read clone format {name!r}: declare the checked language suffixes"
        raise CloneRejected(message)
    return name, tuple(_read_suffix(value=suffix) for suffix in suffixes)


def _read_formats(*, value: object) -> tuple[tuple[str, tuple[str, ...]], ...]:
    if not isinstance(value, dict) or not value:
        message = "read clone formats: declare every checked programming language"
        raise CloneRejected(message)
    return tuple(_read_format(name=name, suffixes=suffixes) for name, suffixes in value.items())


def read_policy(*, path: Path) -> ClonePolicy:
    """Reject unknown fields so limits and path skips have one reviewed owner."""
    value: object = json.loads(path.read_text(encoding="utf-8"))
    match value:
        case {
            "detectorVersion": str() as version,
            "minTokens": minimum_tokens,
            "minLines": minimum_lines,
            "sourceRoots": [*roots],
            "sourceFiles": [*files],
            "formats": formats,
            "extensionlessSources": "shebang",
            "exclusions": [*exclusions],
            **unknown,
        } if version.strip() and roots and not unknown:
            return ClonePolicy(
                version=version,
                minimum_tokens=_read_bound(value=minimum_tokens),
                minimum_lines=_read_bound(value=minimum_lines),
                roots=tuple(_read_path(value=root) for root in roots),
                files=tuple(_read_path(value=file) for file in files),
                formats=_read_formats(value=formats),
                exclusions=tuple(_read_exclusion(value=exclusion) for exclusion in exclusions),
            )
        case _:
            message = (
                f"read clone policy {str(path)!r}: declare detector, bounds, formats, and roots"
            )
            raise CloneRejected(message)
