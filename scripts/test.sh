#!/usr/bin/env bash
# Python tests (against the mock device) + firmware lint for every board. No hardware needed.
set -euo pipefail
ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"

echo "==> python"
(cd "$ROOT_DIR/python" && uv run pytest -q)

"$ROOT_DIR/scripts/lint.sh"
