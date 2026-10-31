"""Enforce declared package ownership with native manifest and source resolvers."""

from dependencies.inspect_dependencies import inspect_dependencies
from dependencies.read_policy import read_policy

__all__ = ["inspect_dependencies", "read_policy"]
