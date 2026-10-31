"""Read optional native source rules without allowing resolver scope overrides."""

import json
from typing import TYPE_CHECKING

from dependencies.errors import DependencyRejected

if TYPE_CHECKING:
    from pathlib import Path


def _read_rule(*, value: object) -> object:
    match value:
        case {"severity": "error"}:
            return value
        case _:
            message = "read source rules: every native forbidden rule must have error severity"
            raise DependencyRejected(message)


def _read_rules(*, value: object, source_rules: str) -> tuple[object, ...]:
    match value:
        case {"forbidden": list() as rules, **unknown} if not unknown:
            return tuple(_read_rule(value=rule) for rule in rules)
        case _:
            message = f"read source rules {source_rules!r}: declare only native forbidden rules"
            raise DependencyRejected(message)


def read_source_rules(*, root: Path, source_rules: str) -> tuple[object, ...]:
    """Keep source-rule selection independent from package-boundary semantics."""
    if not source_rules:
        return ()
    path = (root / source_rules).resolve()
    if not path.is_relative_to(root):
        message = f"read source rules {source_rules!r}: expected a repository path"
        raise DependencyRejected(message)
    value: object = json.loads(path.read_text(encoding="utf-8"))
    return _read_rules(value=value, source_rules=source_rules)
