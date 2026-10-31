"""The detector protocol and source comment markers."""

DETECTOR_NAME = "jscpd"
MARKER_PREFIX = "jscpd"
START_MARKER = f"{MARKER_PREFIX}:ignore-start"
END_MARKER = f"{MARKER_PREFIX}:ignore-end"
MARKER_REASON = " -- duplication -- "
NATIVE_SUFFIXES = {
    "rust": ".rs",
    "python": ".py",
    "typescript": ".ts",
    "tsx": ".tsx",
    "javascript": ".js",
    "jsx": ".jsx",
    "bash": ".sh",
    "css": ".css",
    "scss": ".scss",
}
SHEBANG_FORMATS = (
    ("python", "python"),
    ("uv", "python"),
    ("node", "javascript"),
    ("bash", "bash"),
    ("sh", "bash"),
)
COMMENT_PREFIXES = ("#", "//", "/*", "*", "*/")
PARENT_COMPONENT = ".."
REPORT_NAME = "jscpd-report.json"
SUCCESS_STATUS = 0
CLONES_STATUS = 1
