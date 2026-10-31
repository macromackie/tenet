"""Reject native rule and environment failures even when the JSON reporter exits zero."""

from dependencies.errors import DependencyRejected


def _read_violation(*, value: object) -> str:
    match value:
        case {"from": str() as source, "to": str() as target, "rule": {"name": str() as name}}:
            return f"native source rule {name!r}: {source!r} -> {target!r}"
        case _:
            message = "read dependency report: malformed native violation"
            raise DependencyRejected(message)


def _read_environment_issue(*, value: object) -> str:
    match value:
        case {"name": str() as name, "description": str() as description}:
            return f"dependency resolver environment {name!r}: {description}"
        case _:
            message = "read dependency report: malformed native environment issue"
            raise DependencyRejected(message)


def _read_environment(*, value: object) -> tuple[str, ...]:
    match value:
        case {"issues": list() as issues}:
            return tuple(_read_environment_issue(value=issue) for issue in issues)
        case dict():
            return ()
        case _:
            message = "read dependency report: missing native environment evidence"
            raise DependencyRejected(message)


def read_node_issues(*, value: object) -> tuple[str, ...]:
    """Inspect the native reporter's findings independently of process exit status."""
    match value:
        case {"summary": {"violations": list() as violations, "environment": environment}}:
            return tuple(_read_violation(value=item) for item in violations) + _read_environment(
                value=environment,
            )
        case _:
            message = "read dependency report: missing native violations summary"
            raise DependencyRejected(message)
