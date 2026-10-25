#!/usr/bin/env bash
# Build the Pico W and Pico 2 W firmware (Bluetooth only) as .uf2 files.
#
#   scripts/pico.sh              # both boards, into firmware/pico/dist/
#   scripts/pico.sh OUT_DIR      # somewhere else (scripts/web.sh uses this)
#
# To flash: hold BOOTSEL while plugging the Pico in, then copy the .uf2 onto the
# drive that appears (RPI-RP2 or RP2350). The board restarts into the firmware.
# Needs only stable Rust (rustup adds the two targets from rust-toolchain.toml).
#
# The device name comes from firmware/.env (NXC_NAME), unless NXC_NO_ENV=1.
set -euo pipefail
. "$(dirname "$0")/_env.sh"
[ -z "${NXC_NO_ENV:-}" ] && load_firmware_env --no-wifi

out="${1:-$PICO_DIR/dist}"
mkdir -p "$out"

build() { # chip feature, Rust target, output file
  (cd "$PICO_DIR" && cargo build --release --features "$1" --target "$2")
  python3 "$ROOT_DIR/scripts/uf2.py" "$1" "$PICO_DIR/target/$2/release/nxcontroller-pico" "$out/$3"
}
build rp2040 thumbv6m-none-eabi nxcontroller-pico-w.uf2
build rp2350 thumbv8m.main-none-eabihf nxcontroller-pico2-w.uf2
