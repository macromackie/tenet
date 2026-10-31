"""Reject invalid policies or incomplete dependency evidence."""


class DependencyRejected(Exception):
    """Stop a dependency check that cannot establish the declared boundary."""
