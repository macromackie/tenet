"""Name the Rust syntax forms and literal escapes consumed by source import resolution."""

COMMENT_KINDS = frozenset({"line_comment", "block_comment"})
STRING_KINDS = frozenset({"string_literal", "raw_string_literal"})
MODULE_ROOT_NAMES = frozenset({"lib.rs", "main.rs", "mod.rs"})
INCLUDE_MACROS = frozenset({
    "include",
    "std::include",
    "core::include",
    "::std::include",
    "::core::include",
})
ESCAPES = {
    "\\n": "\n",
    "\\r": "\r",
    "\\t": "\t",
    "\\0": "\0",
    "\\\\": "\\",
    '\\"': '"',
    "\\'": "'",
}

ATTRIBUTE_KIND = "attribute_item"
MODULE_KIND = "mod_item"
MACRO_KIND = "macro_invocation"
MACRO_INPUT_KIND = "token_tree"
IDENTIFIER_KIND = "identifier"
ESCAPE_KIND = "escape_sequence"
PATH_ATTRIBUTE = "path"
INCLUDE_NAME = "include"
ASSIGN_OPERATOR = "="
MACRO_OPERATOR = "!"
RUST_SUFFIX = ".rs"

USE_ALIAS_KIND = "use_as_clause"
