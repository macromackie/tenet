"""Check clones and independently establish that every suppression remains necessary."""

from typing import TYPE_CHECKING

from clones.read_sources import read_sources
from clones.read_suppressions import read_suppressions
from clones.run_detector import run_detector, verify_detector

if TYPE_CHECKING:
    from pathlib import Path

    from clones.types import Clone, ClonePolicy, Fragment, Source, Suppression


def _is_overlapping(*, fragment: Fragment, suppression: Suppression) -> bool:
    return (
        fragment.path == suppression.path
        and fragment.start <= suppression.end
        and fragment.end >= suppression.start
    )


def _describe_clone(*, clone: Clone) -> str:
    first = clone.first
    second = clone.second
    return (
        f"{first.path}:{first.start}-{first.end}: duplication ({clone.tokens} tokens) "
        f"also at {second.path}:{second.start}-{second.end}"
    )


def _read_stale_suppressions(
    *, sources: tuple[Source, ...], policy: ClonePolicy, suppressions: tuple[Suppression, ...]
) -> tuple[str, ...]:
    visible = run_detector(sources=sources, policy=policy, enable_suppressions=False)
    return tuple(
        f"{suppression.path}:{suppression.start}: stale duplication suppression"
        for suppression in suppressions
        if not any(
            _is_overlapping(fragment=fragment, suppression=suppression)
            for clone in visible
            for fragment in (clone.first, clone.second)
        )
    )


def inspect_clones(*, root: Path, policy: ClonePolicy) -> tuple[str, ...]:
    """Report unsuppressed native clones and stale block markers without changing source."""
    verify_detector(policy=policy)
    sources = read_sources(root=root, policy=policy)
    suppressions = tuple(
        region for source in sources for region in read_suppressions(source=source)
    )
    clones = run_detector(sources=sources, policy=policy, enable_suppressions=True)
    errors = [_describe_clone(clone=clone) for clone in clones]
    if suppressions:
        errors.extend(
            _read_stale_suppressions(sources=sources, policy=policy, suppressions=suppressions)
        )
    return tuple(errors)
