pub(super) const COMMON_TERMS: &[&str] = &[
    "arg", "base", "build", "call", "check", "class", "client", "close", "code", "config",
    "context", "core", "count", "create", "data", "default", "entry", "error", "field", "file",
    "flag", "form", "format", "function", "helper", "index", "init", "input", "item", "key", "lib",
    "line", "list", "main", "map", "member", "method", "mode", "module", "name", "note", "object",
    "open", "option", "order", "output", "package", "param", "path", "read", "record", "result",
    "role", "rule", "run", "scope", "set", "shape", "size", "source", "src", "state", "string",
    "style", "test", "text", "type", "util", "value", "write",
];

pub(super) const DECLARATION_KEYWORDS: &[&str] = &[
    "async",
    "class",
    "const",
    "def",
    "default",
    "enum",
    "fn",
    "func",
    "function",
    "interface",
    "let",
    "mod",
    "struct",
    "trait",
    "type",
    "var",
];

pub(super) const REQUIRED_FRAMEWORK_TERMS: &[&str] = &[
    "alembic",
    "celld",
    "clippy",
    "effect",
    "fastapi",
    "oauth",
    "openapi",
    "oxlint",
    "postgres",
    "react",
    "ruff",
    "sql",
    "storybook",
    "webhook",
];

pub(super) const OBSERVATION_PATTERNS: &[(&str, &[&str], &[&str])] = &[
    (
        "explicit input precedence with a default fallback",
        &[],
        &[
            "??",
            ".unwrap_or(",
            ".unwrap_or_else(",
            " is None",
            " is not None",
        ],
    ),
    (
        "borrowed inputs modified in place",
        &[],
        &[
            ".sort(",
            ".splice(",
            ".append(",
            ".extend(",
            ".update(",
            ".sort_unstable(",
        ],
    ),
    (
        "equivalent representations across normalized inputs",
        &[],
        &["os.fspath", "PathLike", ".casefold(", "FromStr"],
    ),
    (
        "idempotent source transformations",
        &["idempotent", "transformation"],
        &[
            "TokenStream",
            "tokenize.",
            "import tokenize",
            "ast.parse(",
            "format_code(",
            "sort_code(",
            "sort_imports(",
            "pretty_print(",
        ],
    ),
    (
        "broad Python exception propagation handler",
        &[],
        &["except Exception", "except:"],
    ),
    (
        "FastAPI routes",
        &["fastapi", "api", "route"],
        &["from fastapi import", "import fastapi"],
    ),
    (
        "React components",
        &["react", "component", "jsx"],
        &["from 'react'", r#"from "react""#],
    ),
    (
        "React hook calls",
        &["react", "hook"],
        &[
            "useEffect(",
            "useState(",
            "useMemo(",
            "useCallback(",
            "useRef(",
        ],
    ),
    (
        "Effect programs",
        &["effect", "schema", "error"],
        &["from 'effect'", r#"from "effect""#],
    ),
    (
        "Rust tracing spans",
        &["tracing", "telemetry", "span"],
        &["tracing::", "use tracing"],
    ),
    (
        "CLI output channels",
        &["cli", "output", "channel"],
        &[
            "process.stdout.write(",
            "process.stderr.write(",
            "io::stdout(",
            "io::stderr(",
            "sys.stdout",
            "sys.stderr",
        ],
    ),
    (
        "SQL database queries",
        &["sql", "database", "store", "schema"],
        &[
            "conn.execute(",
            "cursor.execute(",
            "sqlx::",
            "from sqlalchemy",
            "import sqlalchemy",
        ],
    ),
];
