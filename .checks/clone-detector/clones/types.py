"""Immutable clone policies, inspected sources, and detector findings."""

from dataclasses import dataclass
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from pathlib import Path


@dataclass(frozen=True, kw_only=True)
class ClonePolicy:
    """Declare all source inputs and the detector's smallest reported clone."""

    version: str
    minimum_tokens: int
    minimum_lines: int
    roots: tuple[str, ...]
    files: tuple[str, ...]
    formats: tuple[tuple[str, tuple[str, ...]], ...]
    exclusions: tuple[str, ...]


@dataclass(frozen=True, kw_only=True)
class Source:
    """Retain the original relative path while projecting an explicit native format."""

    path: Path
    content: str
    format: str


@dataclass(frozen=True, kw_only=True)
class Suppression:
    """Name exactly the source lines between one pair of comment markers."""

    path: Path
    start: int
    end: int


@dataclass(frozen=True, kw_only=True)
class StartMarker:
    """Locate a native start marker whose reason has already been validated."""

    line: int


@dataclass(frozen=True, kw_only=True)
class EndMarker:
    """Locate the end of one native suppression block."""

    line: int


@dataclass(frozen=True, kw_only=True)
class Fragment:
    """Locate a cloned fragment in the original repository source."""

    path: Path
    start: int
    end: int


@dataclass(frozen=True, kw_only=True)
class Clone:
    """Preserve both locations and the native detector's token count."""

    first: Fragment
    second: Fragment
    tokens: int
