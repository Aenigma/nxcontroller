#!/usr/bin/env bash
# Python tests (against the mock device) + firmware compile check. No hardware needed.
set -euo pipefail
ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"

echo "==> python"
(cd "$ROOT_DIR/python" && uv run pytest -q)

echo "==> firmware (clippy)"
. "$ROOT_DIR/scripts/_env.sh"
cd "$FIRMWARE_DIR"
WIFI_SSID="${WIFI_SSID:-ci}" WIFI_PASSWORD="${WIFI_PASSWORD:-ci}" cargo clippy --release -- -D warnings
