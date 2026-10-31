"""Name manifest formats and the source suffixes the standard resolver accepts."""

SOURCE_SUFFIXES = frozenset({".ts", ".tsx", ".mts", ".cts", ".js", ".jsx", ".mjs", ".cjs"})
MANIFEST_NAMES = frozenset({"Cargo.toml", "package.json"})
CARGO_MANIFEST = "Cargo.toml"
CARGO_MANAGER = "cargo"
NODE_MANIFEST = "package.json"
NODE_MANAGER = "npm"
PARENT_COMPONENT = ".."
NEWLINE = "\n"

NODE_GRAPH_TIMEOUT_SECONDS = 180
NODE_MODULES_DIRECTORY = "node_modules"
