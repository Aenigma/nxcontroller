#!/usr/bin/env bash
# Python tests (against the mock device) + firmware compile check. No hardware needed.
set -euo pipefail
ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"

echo "==> python"
(cd "$ROOT_DIR/python" && uv run pytest -q)

. "$ROOT_DIR/scripts/_env.sh"
cd "$FIRMWARE_DIR"
for features in "" "--no-default-features --features ble" "--no-default-features --features wifi"; do
  echo "==> firmware clippy ${features:-(default: ble + wifi)}"
  # shellcheck disable=SC2086
  WIFI_SSID="${WIFI_SSID:-ci}" WIFI_PASSWORD="${WIFI_PASSWORD:-ci}" cargo clippy --release $features -- -D warnings
done
