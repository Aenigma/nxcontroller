#!/usr/bin/env bash
# Python tests (against the mock device) + firmware lint for every board. No hardware needed.
set -euo pipefail
ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"

echo "==> python"
(cd "$ROOT_DIR/python" && uv run pytest -q)

. "$ROOT_DIR/scripts/_env.sh"

# The core on its own (as a dependency of a board crate it isn't linted).
for features in ble wifi ble,wifi; do
  echo "==> core clippy ($features)"
  (cd "$ROOT_DIR/firmware/core" && cargo clippy --release --features "$features" --target thumbv6m-none-eabi -- -D warnings)
done

for chip in "rp2040 thumbv6m-none-eabi" "rp2350 thumbv8m.main-none-eabihf"; do
  set -- $chip
  echo "==> pico clippy ($1)"
  (cd "$PICO_DIR" && cargo clippy --release --features "$1" --target "$2" -- -D warnings)
done

use_esp_toolchain
cd "$ESP_DIR"
for features in "" "--no-default-features --features ble" "--no-default-features --features wifi"; do
  echo "==> esp32s3 clippy ${features:-(default: ble + wifi)}"
  # shellcheck disable=SC2086
  WIFI_SSID="${WIFI_SSID:-ci}" WIFI_PASSWORD="${WIFI_PASSWORD:-ci}" cargo clippy --release $features -- -D warnings
done
