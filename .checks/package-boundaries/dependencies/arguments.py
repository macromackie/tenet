"""Read the checker's explicit root and policy locations."""

import argparse


def read_arguments() -> argparse.Namespace:
    """Keep optional consumer source rules separate from package ownership."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", required=True, help="repository root to inspect")
    parser.add_argument("--config", required=True, help="package policy relative to that root")
    parser.add_argument(
        "--source-rules", default="", help="optional consumer-owned native source rules"
    )
    parser.add_argument(
        "--verify-projection", default="", help="derived standalone policy to verify"
    )
    parser.add_argument(
        "--update-projection", action="store_true", help="regenerate that derived policy"
    )
    return parser.parse_args()
