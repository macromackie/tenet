"""Decode Rust's cooked and raw path literals without evaluating expressions."""

from typing import TYPE_CHECKING

from dependencies.errors import DependencyRejected
from dependencies.rust.constants import ESCAPE_KIND, ESCAPES, STRING_KINDS

if TYPE_CHECKING:
    from tree_sitter import Node


def _read_escape(*, value: str) -> str:
    if value in ESCAPES:
        return ESCAPES[value]
    if value.startswith("\\x"):
        return chr(int(value.removeprefix("\\x"), 16))
    if value.startswith("\\u{") and value.endswith("}"):
        return chr(int(value[3:-1].replace("_", ""), 16))
    if value.startswith("\\") and value[1:].isspace():
        return ""
    message = f"read Rust source path escape {value!r}: unsupported literal escape"
    raise DependencyRejected(message)


def _read_part(*, node: Node) -> str:
    content = node.text
    if content is None:
        message = "read Rust source path: the parser did not retain literal bytes"
        raise DependencyRejected(message)
    value = content.decode("utf-8")
    if node.type == ESCAPE_KIND:
        return _read_escape(value=value)
    return value


def read_literal(*, node: Node) -> str:
    """Retain raw UTF-8 and decode each syntactically parsed string escape."""
    if node.type not in STRING_KINDS:
        message = f"read Rust source path: expected a literal string, received {node.type!r}"
        raise DependencyRejected(message)
    return "".join(_read_part(node=child) for child in node.named_children)
