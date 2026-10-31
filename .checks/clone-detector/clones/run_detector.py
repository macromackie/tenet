"""Run the pinned native detector over complete, temporary source projections."""

import json
import subprocess
from pathlib import Path
from tempfile import TemporaryDirectory
from typing import TYPE_CHECKING

from clones.constants import (
    CLONES_STATUS,
    DETECTOR_NAME,
    END_MARKER,
    NATIVE_SUFFIXES,
    REPORT_NAME,
    START_MARKER,
    SUCCESS_STATUS,
)
from clones.errors import CloneRejected
from clones.types import Clone, Fragment

if TYPE_CHECKING:
    from collections.abc import Mapping

    from clones.types import ClonePolicy, Source


def _read_fragment(*, value: object, paths: Mapping[str, Path]) -> Fragment:
    match value:
        case {"name": str() as name, "start": int() as start, "end": int() as end} if (
            name in paths and start > 0 and end >= start
        ):
            return Fragment(path=paths[name], start=start, end=end)
        case _:
            message = f"read clone fragment {value!r}: expected a projected source location"
            raise CloneRejected(message)


def _read_clones(*, report: Path, paths: Mapping[str, Path]) -> tuple[Clone, ...]:
    value: object = json.loads(report.read_text(encoding="utf-8"))
    match value:
        case {"duplicates": list() as duplicates}:
            return tuple(_read_clone(value=duplicate, paths=paths) for duplicate in duplicates)
        case _:
            message = "read clone report: expected the native detector's duplicates array"
            raise CloneRejected(message)


def _read_clone(*, value: object, paths: Mapping[str, Path]) -> Clone:
    match value:
        case {"firstFile": first, "secondFile": second, "tokens": int() as tokens}:
            return Clone(
                first=_read_fragment(value=first, paths=paths),
                second=_read_fragment(value=second, paths=paths),
                tokens=tokens,
            )
        case _:
            message = f"read clone result {value!r}: expected two source fragments"
            raise CloneRejected(message)


def verify_detector(*, policy: ClonePolicy) -> None:
    """Refuse a different detector release before its output becomes check evidence."""
    result = subprocess.run(
        [DETECTOR_NAME, "--version"], check=True, capture_output=True, text=True
    )
    expected = f"{DETECTOR_NAME} {policy.version}"
    if result.stdout.strip() != expected:
        message = f"run clone detector: expected {expected!r}, received {result.stdout.strip()!r}"
        raise CloneRejected(message)


def _write_projection(
    *, directory: Path, sources: tuple[Source, ...], enable_suppressions: bool
) -> dict[str, Path]:
    directory.mkdir()
    paths: dict[str, Path] = {}
    for index, source in enumerate(sources):
        name = f"source-{index}{NATIVE_SUFFIXES[source.format]}"
        content = source.content
        if not enable_suppressions:
            content = content.replace(START_MARKER, "clone-check-start").replace(
                END_MARKER, "clone-check-end"
            )
        (directory / name).write_text(content, encoding="utf-8")
        paths[name] = source.path
    return paths


def _write_configuration(
    *, directory: Path, sources: tuple[Source, ...], policy: ClonePolicy
) -> Path:
    native_configuration = {
        "minTokens": policy.minimum_tokens,
        "minLines": policy.minimum_lines,
        "maxSize": str(max(len(source.content.encode("utf-8")) for source in sources) + 1),
        "threshold": 0,
        "gitignore": False,
        "mode": "mild",
        "reporters": ["json"],
    }
    configuration = directory / "jscpd.json"
    configuration.write_text(json.dumps(native_configuration), encoding="utf-8")
    return configuration


def _run_native(*, configuration: Path, reports: Path, projection: Path) -> None:
    result = subprocess.run(
        [
            DETECTOR_NAME,
            "--config",
            str(configuration),
            "--no-gitignore",
            "--exit-code",
            str(CLONES_STATUS),
            "--output",
            str(reports),
            str(projection),
        ],
        check=False,
        capture_output=True,
        text=True,
    )
    if result.returncode not in {SUCCESS_STATUS, CLONES_STATUS}:
        message = f"run clone detector: status {result.returncode}: {result.stderr.strip()}"
        raise CloneRejected(message)
    if not (reports / REPORT_NAME).is_file():
        message = f"run clone detector: missing JSON report: {result.stderr.strip()}"
        raise CloneRejected(message)


def run_detector(
    *, sources: tuple[Source, ...], policy: ClonePolicy, enable_suppressions: bool
) -> tuple[Clone, ...]:
    """Compute the size ceiling from all inputs so the detector skips no source length."""
    with TemporaryDirectory(prefix="jscpd-check-") as temporary:
        directory = Path(temporary)
        projection = directory / "source"
        paths = _write_projection(
            directory=projection, sources=sources, enable_suppressions=enable_suppressions
        )
        configuration = _write_configuration(directory=directory, sources=sources, policy=policy)
        reports = directory / "report"
        _run_native(configuration=configuration, reports=reports, projection=projection)
        return _read_clones(report=reports / REPORT_NAME, paths=paths)
