# Shared setup for firmware scripts. Sourced, not executed.
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
FIRMWARE_DIR="$ROOT_DIR/firmware"
ELF="$FIRMWARE_DIR/target/xtensa-esp32s3-none-elf/release/nxcontroller-firmware"

# Xtensa toolchain env from espup (adds the linker + libclang to PATH).
if ! command -v xtensa-esp32s3-elf-gcc >/dev/null 2>&1; then
  if [ -f "$HOME/export-esp.sh" ]; then
    # shellcheck disable=SC1091
    . "$HOME/export-esp.sh"
  else
    echo "Xtensa toolchain not found. Run scripts/setup.sh first." >&2
    exit 1
  fi
fi

load_firmware_env() {
  if [ -f "$FIRMWARE_DIR/.env" ]; then
    set -a
    # shellcheck disable=SC1091
    . "$FIRMWARE_DIR/.env"
    set +a
  fi
  if [ -z "${WIFI_SSID:-}" ] || [ -z "${WIFI_PASSWORD+x}" ]; then
    echo "WIFI_SSID / WIFI_PASSWORD not set. Copy firmware/.env.example to firmware/.env and fill it in." >&2
    exit 1
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
