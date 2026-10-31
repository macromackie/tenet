"""Verify that a standalone policy is derived from its containing repository's declaration."""

import json
from pathlib import PurePosixPath
from typing import TYPE_CHECKING

from dependencies.errors import DependencyRejected

if TYPE_CHECKING:
    from pathlib import Path

    from dependencies.model import Package, Policy


def _read_relative(*, value: str, prefix: str) -> str:
    if not value.startswith(prefix):
        message = f"project package path {value!r}: dependency leaves the exported scope {prefix!r}"
        raise DependencyRejected(message)
    return value.removeprefix(prefix)


def _read_package_projection(*, package: Package, prefix: str) -> dict[str, object]:
    return {
        "name": package.name,
        "manifest": _read_relative(value=package.manifest, prefix=prefix),
        "manager": package.manager,
        "ownership": package.ownership,
        "dependencies": [
            {
                "manifest": _read_relative(value=edge.target, prefix=prefix),
                "testOnly": edge.test_only,
            }
            for edge in package.dependencies
        ],
        "testSources": list(package.test_sources),
        "testSupport": package.test_support,
        "tsconfig": _read_relative(value=package.tsconfig, prefix=prefix)
        if package.tsconfig
        else "",
        "builtins": list(package.builtins),
    }


def _read_exclusion_projection(*, value: object, prefix: str) -> tuple[dict[str, str], ...]:
    match value:
        case {"pattern": str() as pattern, "reason": str() as reason}:
            if pattern.startswith(prefix):
                return ({"pattern": pattern.removeprefix(prefix), "reason": reason},)
            if pattern.startswith("**/"):
                return ({"pattern": pattern, "reason": reason},)
            return ()
        case _:
            message = "project package exclusions: expected validated patterns and reasons"
            raise DependencyRejected(message)


def _read_exclusions(*, value: object, prefix: str) -> tuple[dict[str, str], ...]:
    match value:
        case {"exclusions": [*exclusions]}:
            return tuple(
                projected
                for exclusion in exclusions
                for projected in _read_exclusion_projection(value=exclusion, prefix=prefix)
            )
        case _:
            message = "project package policy: expected its validated exclusions"
            raise DependencyRejected(message)


def _read_projection(
    *, root: Path, source: str, destination: str, policy: Policy
) -> dict[str, object]:
    scope = PurePosixPath(destination).parent
    prefix = "" if scope == PurePosixPath() else f"{scope.as_posix()}/"
    packages = tuple(package for package in policy.packages if package.manifest.startswith(prefix))
    if not packages:
        message = f"project package policy {destination!r}: scope contains no declared packages"
        raise DependencyRejected(message)
    original: object = json.loads((root / source).read_text(encoding="utf-8"))
    return {
        "packages": [
            _read_package_projection(package=package, prefix=prefix) for package in packages
        ],
        "exclusions": list(_read_exclusions(value=original, prefix=prefix)),
    }


def _is_requested(*, destination: str, update: bool) -> bool:
    if not destination:
        if update:
            message = "update package projection: declare --verify-projection first"
            raise DependencyRejected(message)
        return False
    return True


def _read_destination(*, root: Path, destination: str) -> Path:
    target = (root / destination).resolve()
    if not target.is_relative_to(root):
        message = "project package policy: destination must stay inside the root"
        raise DependencyRejected(message)
    return target


def inspect_projection(
    *, root: Path, source: str, destination: str, policy: Policy, update: bool
) -> tuple[str, ...]:
    """Require an exact generated view without adding another dependency authority."""
    if not _is_requested(destination=destination, update=update):
        return ()
    target = _read_destination(root=root, destination=destination)
    expected = _read_projection(root=root, source=source, destination=destination, policy=policy)
    if update:
        target.write_text(json.dumps(expected, indent=2) + "\n", encoding="utf-8")
    actual: object = json.loads(target.read_text(encoding="utf-8"))
    if actual != expected:
        return (f"{destination}: stale package projection; regenerate with --update-projection",)
    return ()
