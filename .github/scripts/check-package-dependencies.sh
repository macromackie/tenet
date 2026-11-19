#!/usr/bin/env bash
set -euo pipefail

shasum -a 256 --check .checks/package-boundaries/SHA256SUMS
shasum -a 256 --check .checks/npm/SHA256SUMS
npm --prefix .checks/npm ci --ignore-scripts --no-fund --no-audit
npm --prefix checker ci --ignore-scripts --no-fund --no-audit
exec uv run --locked --script .checks/package-boundaries/check-package-boundaries.py \
  --root . --config package-boundaries.json
