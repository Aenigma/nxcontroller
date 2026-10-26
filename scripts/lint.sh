#!/usr/bin/env bash
# Clippy on the firmware, warnings as errors. No hardware needed.
#
#   scripts/lint.sh                  # everything
#   scripts/lint.sh core pico        # stable Rust only (what CI's Pico job runs)
#   scripts/lint.sh esp32s3          # the ESP32-S3 board (Xtensa toolchain)
set -euo pipefail
. "$(dirname "$0")/_env.sh"

parts=("$@")
[ $# -eq 0 ] && parts=(core pico esp32s3)
for part in "${parts[@]}"; do
    case "$part" in
      core)
        # On its own: as a board's dependency it isn't linted.
        for features in ble wifi ble,wifi; do
          echo "==> core clippy ($features)"
          (cd "$ROOT_DIR/firmware/core" && cargo clippy --release --features "$features" --target thumbv6m-none-eabi -- -D warnings)
        done ;;
      pico)
        for chip in "${PICO_CHIPS[@]}"; do
          read -r feature target _ <<<"$chip"
          echo "==> pico clippy ($feature)"
          (cd "$PICO_DIR" && cargo clippy --release --features "$feature" --target "$target" -- -D warnings)
        done ;;
      esp32s3)
        use_esp_toolchain
        for features in "" "--no-default-features --features ble" "--no-default-features --features wifi"; do
          echo "==> esp32s3 clippy ${features:-(default: ble + wifi)}"
          # shellcheck disable=SC2086
          (cd "$ESP_DIR" && WIFI_SSID="${WIFI_SSID:-ci}" WIFI_PASSWORD="${WIFI_PASSWORD:-ci}" cargo clippy --release $features -- -D warnings)
        done ;;
      *) echo "unknown part '$part' (core, pico, esp32s3)" >&2; exit 1 ;;
    esac
done
