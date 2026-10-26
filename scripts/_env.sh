# Shared setup for firmware scripts. Sourced, not executed.
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ENV_FILE="$ROOT_DIR/firmware/.env"
ESP_DIR="$ROOT_DIR/firmware/esp32s3"   # ESP32-S3 board crate (Xtensa toolchain)
PICO_DIR="$ROOT_DIR/firmware/pico"     # Pico W / Pico 2 W board crate (stable Rust)
ELF="$ESP_DIR/target/xtensa-esp32s3-none-elf/release/nxcontroller-firmware"

# Pico boards: cargo feature, Rust target, .uf2 name (web/src/components/Install.tsx links to these).
PICO_CHIPS=(
  "rp2040 thumbv6m-none-eabi nxcontroller-pico-w.uf2"
  "rp2350 thumbv8m.main-none-eabihf nxcontroller-pico2-w.uf2"
)

# Builds every Pico board and writes its .uf2 into the directory given.
build_pico_uf2() {
  local out="$1" chip feature target file
  mkdir -p "$out"
  for chip in "${PICO_CHIPS[@]}"; do
    read -r feature target file <<<"$chip"
    (cd "$PICO_DIR" && cargo build --release --features "$feature" --target "$target")
    python3 "$ROOT_DIR/scripts/uf2.py" "$feature" "$ROOT_DIR/firmware/target/$target/release/nxcontroller-pico" "$out/$file"
  done
}

# Xtensa toolchain env from espup (adds the linker + libclang to PATH).
use_esp_toolchain() {
  command -v xtensa-esp32s3-elf-gcc >/dev/null 2>&1 && return
  if [ -f "$HOME/export-esp.sh" ]; then
    # shellcheck disable=SC1091
    . "$HOME/export-esp.sh"
  else
    echo "Xtensa toolchain not found. Run scripts/setup.sh first." >&2
    exit 1
  fi
}

# firmware/.env: device name, plus WiFi for ESP32-S3 builds with WiFi.
load_firmware_env() {
  if [ -f "$ENV_FILE" ]; then
    set -a
    # shellcheck disable=SC1091
    . "$ENV_FILE"
    set +a
  fi
}

# For ESP32-S3 builds with WiFi.
warn_no_wifi() {
  if [ -z "${WIFI_SSID:-}" ]; then
    echo "note: no WIFI_SSID in firmware/.env; the board will start the 'nxcontroller-setup' hotspot" >&2
    echo "      unless it already has WiFi saved (set it there, or with \`nxc wifi\`)." >&2
  fi
}

# Native USB port while the ROM is in download mode (Espressif USB-Serial-JTAG, 303A:1001).
find_jtag_port() {
  espflash list-ports 2>/dev/null | awk '/303A/ && ($1 ~ /\/dev\/(cu\.|ttyACM)/) {print $1; exit}'
}

# UART bridge port (CH343 / CP210x / FTDI). Picks the first serial device that
# isn't the native port; override with --monitor-port or ESPFLASH_MONITOR_PORT.
find_uart_port() {
  local jtag p
  jtag="$(find_jtag_port)"
  for p in /dev/cu.usbserial* /dev/cu.wchusbserial* /dev/cu.SLAB_USBtoUART* /dev/cu.usbmodem* /dev/ttyUSB* /dev/ttyACM*; do
    [ -e "$p" ] && [ "$p" != "$jtag" ] && { echo "$p"; return; }
  done
}
