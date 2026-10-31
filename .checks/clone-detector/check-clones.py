#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.14"
# dependencies = ["jscpd==5.4.2"]
# ///
"""Check all declared source clones and reject stale or overbroad block suppressions."""

import argparse
from pathlib import Path

from clones import inspect_clones, read_policy
from clones.errors import CloneRejected


def main() -> None:
    """Keep the detector version, source scope, and clone limits in one checked policy."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", required=True, help="repository root to inspect")
    parser.add_argument("--config", required=True, help="clone policy relative to that root")
    arguments = parser.parse_args()
    root = Path(arguments.root).resolve()
    policy = read_policy(path=root / arguments.config)
    errors = inspect_clones(root=root, policy=policy)
    if errors:
        raise CloneRejected("\n".join(errors))


if __name__ == "__main__":
    main()
