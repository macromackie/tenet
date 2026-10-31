"""Require local paired clone markers with a reason and source outside their block."""

from itertools import batched
from typing import TYPE_CHECKING

from clones.constants import COMMENT_PREFIXES, END_MARKER, MARKER_REASON, START_MARKER
from clones.errors import CloneRejected
from clones.types import EndMarker, StartMarker, Suppression

if TYPE_CHECKING:
    from clones.types import Source


def _read_marker(*, source: Source, line: str, number: int) -> StartMarker | EndMarker:
    prefix = line.split("jscpd:", maxsplit=1)[0].strip()
    if prefix not in COMMENT_PREFIXES:
        message = f"{source.path}:{number}: clone markers must occupy their own comment line"
        raise CloneRejected(message)
    if START_MARKER in line:
        reason = line.partition(START_MARKER)[2].strip()
        if (
            not reason.startswith(MARKER_REASON.strip())
            or not reason.removeprefix(MARKER_REASON.strip()).strip()
        ):
            message = f"{source.path}:{number}: clone start requires one duplication reason"
            raise CloneRejected(message)
        return StartMarker(line=number)
    return EndMarker(line=number)


def _read_regions(
    *, source: Source, markers: tuple[StartMarker | EndMarker, ...]
) -> tuple[Suppression, ...]:
    regions: list[Suppression] = []
    for pair in batched(markers, 2, strict=False):
        match pair:
            case (StartMarker(line=start), EndMarker(line=end)) if end > start + 1:
                regions.append(Suppression(path=source.path, start=start + 1, end=end - 1))
            case _:
                message = f"{source.path}: clone markers must pair one start and one nonempty end"
                raise CloneRejected(message)
    return tuple(regions)


def _check_region(*, source: Source, region: Suppression) -> None:
    lines = source.content.splitlines()
    outside = lines[: region.start - 2] + lines[region.end + 1 :]
    if not any(line.strip() and not line.lstrip().startswith(COMMENT_PREFIXES) for line in outside):
        message = f"{source.path}:{region.start}: clone markers cover the whole source file"
        raise CloneRejected(message)


def read_suppressions(*, source: Source) -> tuple[Suppression, ...]:
    """Reject nested, unpaired, reasonless, and whole-file native suppressions."""
    markers = tuple(
        _read_marker(source=source, line=line, number=number)
        for number, line in enumerate(source.content.splitlines(), start=1)
        if START_MARKER in line or END_MARKER in line
    )
    regions = _read_regions(source=source, markers=markers)
    for region in regions:
        _check_region(source=source, region=region)
    return regions
