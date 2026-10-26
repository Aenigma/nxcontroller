#!/usr/bin/env bash
# Build the ESP32-S3 firmware (release). Extra args go to cargo.
# For the Pico W / Pico 2 W, see scripts/pico.sh.
set -euo pipefail
. "$(dirname "$0")/_env.sh"
use_esp_toolchain
load_firmware_env
warn_no_wifi

cd "$ESP_DIR"
cargo build --release "$@"
echo "Built $ELF"
