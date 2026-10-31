"""Decode a closed package policy before invoking any tools."""

import json
from pathlib import PurePosixPath
from typing import TYPE_CHECKING

from dependencies.constants import CARGO_MANAGER, NEWLINE, NODE_MANAGER, PARENT_COMPONENT
from dependencies.errors import DependencyRejected
from dependencies.model import AllowedDependency, Package, Policy

if TYPE_CHECKING:
    from pathlib import Path
    from typing import Literal


def _read_path(*, value: object) -> str:
    if not isinstance(value, str) or not value.strip():
        message = f"read dependency path {value!r}: expected a nonempty relative path"
        raise DependencyRejected(message)
    path = PurePosixPath(value)
    if path.is_absolute() or PARENT_COMPONENT in path.parts:
        message = f"read dependency path {value!r}: expected a path inside the repository"
        raise DependencyRejected(message)
    return value


def _read_paths(*, value: object) -> tuple[str, ...]:
    if not isinstance(value, list):
        message = f"read dependency paths {value!r}: expected a list"
        raise DependencyRejected(message)
    return tuple(_read_path(value=entry) for entry in value)


def _read_dependency(*, value: object) -> AllowedDependency:
    match value:
        case {"manifest": target, "testOnly": bool() as test_only, **unknown} if not unknown:
            return AllowedDependency(target=_read_path(value=target), test_only=test_only)
        case _:
            message = f"read allowed dependency {value!r}: declare its manifest and testOnly flag"
            raise DependencyRejected(message)


def _read_manager(*, value: object) -> Literal["cargo", "npm"]:
    match value:
        case "cargo":
            return CARGO_MANAGER
        case "npm":
            return NODE_MANAGER
        case _:
            message = f"read package manager {value!r}: expected a supported package manager"
            raise DependencyRejected(message)


def _read_package(*, value: object) -> Package:
    match value:
        case {
            "name": str() as name,
            "manifest": manifest,
            "manager": manager,
            "ownership": str() as ownership,
            "dependencies": [*dependencies],
            "testSources": test_sources,
            "testSupport": bool() as test_support,
            "tsconfig": str() as tsconfig,
            "builtins": [*builtins],
            **unknown,
        } if name.strip() and ownership.strip() and NEWLINE not in ownership and not unknown:
            return Package(
                name=name,
                manifest=_read_path(value=manifest),
                manager=_read_manager(value=manager),
                ownership=ownership,
                dependencies=tuple(_read_dependency(value=edge) for edge in dependencies),
                test_sources=_read_paths(value=test_sources),
                test_support=test_support,
                tsconfig=_read_path(value=tsconfig) if tsconfig else "",
                builtins=tuple(_read_path(value=builtin) for builtin in builtins),
            )
        case _:
            message = f"read package declaration {value!r}: expected a complete ownership record"
            raise DependencyRejected(message)


def _read_exclusion(*, value: object) -> str:
    match value:
        case {"pattern": pattern, "reason": str() as reason, **unknown} if (
            reason.strip() and not unknown
        ):
            return _read_path(value=pattern)
        case _:
            message = f"read package exclusion {value!r}: declare a pattern and its reason"
            raise DependencyRejected(message)


def read_policy(*, path: Path) -> Policy:
    """Reject unknown options rather than accepting hidden scope or edge exceptions."""
    value: object = json.loads(path.read_text(encoding="utf-8"))
    match value:
        case {"packages": [*packages], "exclusions": [*exclusions], **unknown} if (
            packages and not unknown
        ):
            return Policy(
                packages=tuple(_read_package(value=package) for package in packages),
                exclusions=tuple(_read_exclusion(value=exclusion) for exclusion in exclusions),
            )
        case _:
            message = f"read dependency policy {str(path)!r}: declare packages and exclusions"
            raise DependencyRejected(message)
