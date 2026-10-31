"""Reject contradictory ownership declarations before checking their observed edges."""

from graphlib import TopologicalSorter
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from dependencies.model import Package, Policy


def _validate_dependencies(*, package: Package, members: dict[str, Package]) -> tuple[str, ...]:
    errors: list[str] = []
    targets: set[str] = set()
    for dependency in package.dependencies:
        if dependency.target in targets:
            errors.append(f"{package.manifest}: repeated dependency {dependency.target!r}")
        targets.add(dependency.target)
        if dependency.target not in members:
            errors.append(f"{package.manifest}: unknown dependency {dependency.target!r}")
        elif dependency.test_only and not members[dependency.target].test_support:
            errors.append(
                f"{package.manifest}: test-only edge targets a package "
                f"not declared as test support: "
                f"{dependency.target!r}"
            )
    return tuple(errors)


def validate_policy(*, policy: Policy) -> tuple[str, ...]:
    """Require one owner per package and an acyclic, explicit internal graph."""
    members = {package.manifest: package for package in policy.packages}
    errors: list[str] = []
    if len(members) != len(policy.packages):
        errors.append("dependency policy repeats a package manifest")
    for package in policy.packages:
        errors.extend(_validate_dependencies(package=package, members=members))
    if errors:
        return tuple(errors)
    edges = {
        package.manifest: {
            dependency.target for dependency in package.dependencies if not dependency.test_only
        }
        for package in policy.packages
    }
    tuple(TopologicalSorter(edges).static_order())
    return ()
