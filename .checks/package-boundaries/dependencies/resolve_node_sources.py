"""Resolve source imports with the pinned dependency-cruiser and TypeScript parser."""

import json
import os
import subprocess
from pathlib import Path
from tempfile import TemporaryDirectory
from typing import TYPE_CHECKING

from dependencies.constants import NODE_GRAPH_TIMEOUT_SECONDS
from dependencies.errors import DependencyRejected
from dependencies.node_report import read_node_report

if TYPE_CHECKING:
    from dependencies.model import Graph, NodeProject


def _read_pin(*, tools: Path) -> str:
    value: object = json.loads((tools / "package.json").read_text(encoding="utf-8"))
    match value:
        case {"dependencies": {"dependency-cruiser": str() as version}}:
            return version
        case _:
            message = "read dependency resolver: expected the shipped exact tool pin"
            raise DependencyRejected(message)


def _verify_resolver(*, tools: Path) -> Path:
    expected = _read_pin(tools=tools)
    executable = tools / "node_modules/.bin/depcruise"
    result = subprocess.run(
        [str(executable), "--version"],
        check=True,
        capture_output=True,
        text=True,
        timeout=NODE_GRAPH_TIMEOUT_SECONDS,
    )
    if result.stdout.strip() != expected:
        message = (
            f"run dependency resolver: expected {expected!r}, received {result.stdout.strip()!r}"
        )
        raise DependencyRejected(message)
    return executable


def _write_configuration(
    *,
    directory: Path,
    project: NodeProject,
    rules: tuple[object, ...],
) -> Path:
    configuration = {
        "forbidden": list(rules),
        "options": {
            "parser": "tsc",
            "tsPreCompilationDeps": True,
            "doNotFollow": {"path": "(^|/)node_modules/"},
            "builtInModules": {"add": list(project.builtins)},
            "enhancedResolveOptions": {
                "exportsFields": ["exports"],
                "mainFields": ["types", "typings", "module", "main"],
                "conditionNames": ["types", "import", "require", "node", "default"],
            },
        },
    }
    path = directory / "resolver.json"
    path.write_text(json.dumps(configuration), encoding="utf-8")
    return path


def resolve_node_sources(
    *,
    root: Path,
    project: NodeProject,
    exclusions: tuple[str, ...],
    rules: tuple[object, ...],
) -> Graph:
    """Keep type-only and dynamic imports while following workspace package exports."""
    executable = _verify_resolver(tools=Path(__file__).resolve().parents[2] / "npm")
    directory = (root / project.manifest).parent
    with TemporaryDirectory(prefix="package-dependencies-") as temporary:
        configuration = _write_configuration(
            directory=Path(temporary), project=project, rules=rules
        )
        arguments = [
            str(executable),
            "--config",
            str(configuration),
            "--no-cache",
            "--output-type",
            "json",
        ]
        if project.tsconfig:
            arguments.extend(["--ts-config", os.path.relpath(root / project.tsconfig, directory)])
        arguments.extend(os.path.relpath(root / source, directory) for source in project.sources)
        result = subprocess.run(
            arguments,
            cwd=directory,
            check=True,
            capture_output=True,
            text=True,
            timeout=NODE_GRAPH_TIMEOUT_SECONDS,
        )
    return read_node_report(
        root=root,
        project=project,
        content=result.stdout,
        exclusions=exclusions,
    )
