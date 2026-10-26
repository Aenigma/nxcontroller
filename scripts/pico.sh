#!/usr/bin/env bash
# Build the Pico W and Pico 2 W firmware (Bluetooth only) as .uf2 files.
#
#   scripts/pico.sh              # both boards, into firmware/pico/dist/
#   scripts/pico.sh OUT_DIR      # somewhere else
#
# To flash: hold BOOTSEL while plugging the Pico in, then copy the .uf2 onto the
# drive that appears (RPI-RP2 or RP2350). The board restarts into the firmware.
# Needs only stable Rust (rustup adds the two targets from rust-toolchain.toml).
# The device name comes from firmware/.env (NXC_NAME).
set -euo pipefail
. "$(dirname "$0")/_env.sh"
load_firmware_env
build_pico_uf2 "${1:-$PICO_DIR/dist}"
