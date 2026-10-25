#!/usr/bin/env bash
# Build, flash and (by default) show the serial log.
#
#   scripts/flash.sh                       # build, flash, monitor
#   scripts/flash.sh --no-monitor
#   scripts/flash.sh -p PORT               # flash through PORT with espflash's own reset logic
#   scripts/flash.sh --monitor-port PORT   # where the UART log lives (default: auto)
#
# Default flow (DevKitC-style board, both ports plugged into this computer):
#   1. Flash through the NATIVE USB port, with the ROM in download mode. Once our
#      firmware runs, that port is a gamepad/keyboard, so the script asks you
#      to hold BOOT and tap RST.
#   2. Reset and read the log through the UART port (CH343/CP210x bridge).
#      Writing large blocks through macOS's CH343 driver fails, but reading works.
set -euo pipefail

monitor=1
flash_port=""
monitor_port="${ESPFLASH_MONITOR_PORT:-}"
cargo_args=()
while [ $# -gt 0 ]; do
  case "$1" in
    -p|--port) flash_port="$2"; shift 2 ;;
    --monitor-port) monitor_port="$2"; shift 2 ;;
    --no-monitor) monitor=0; shift ;;
    --gamepad-only|--keyboard-only)
      echo "$1 is gone: the USB mode is now chosen at runtime, e.g. \`nxc mode keyboard\`" >&2; exit 2 ;;
    -h|--help) sed -n '2,15p' "$0"; exit 0 ;;
    *) cargo_args+=("$1"); shift ;;
  esac
done

. "$(dirname "$0")/_env.sh"
use_esp_toolchain
load_firmware_env

(cd "$ESP_DIR" && cargo build --release ${cargo_args[@]+"${cargo_args[@]}"})

# Kill espflash's own process if it hangs on a wedged port.
with_timeout() { perl -e 'alarm shift; exec @ARGV' "$@"; }

if [ -n "$flash_port" ]; then
  with_timeout 120 espflash flash --chip esp32s3 --non-interactive --port "$flash_port" "$ELF"
else
  jtag_port="$(find_jtag_port)"
  if [ -z "$jtag_port" ]; then
    echo
    echo ">>> Put the board in download mode: hold BOOT, tap RST, release BOOT."
    echo "    (the native USB port must be plugged into this computer)"
    for _ in $(seq 60); do
      sleep 1
      jtag_port="$(find_jtag_port)"
      [ -n "$jtag_port" ] && break
    done
    if [ -z "$jtag_port" ]; then
      echo "Board did not show up in download mode." >&2
      exit 1
    fi
  fi
  echo "==> flashing via $jtag_port"
  with_timeout 120 espflash flash --chip esp32s3 --non-interactive \
    --before no-reset --after no-reset --port "$jtag_port" "$ELF"
fi

[ -n "$monitor_port" ] || monitor_port="$(find_uart_port)"
if [ -z "$monitor_port" ]; then
  echo "Flashed. No UART port found: press RST to start the firmware."
  exit 0
fi

if [ "$monitor" = 1 ]; then
  # Opening the monitor toggles DTR/RTS, which resets the chip into the new firmware.
  exec espflash monitor --chip esp32s3 --no-stub --port "$monitor_port" --elf "$ELF"
else
  # Short-lived monitor = reset via UART, then exit.
  espflash monitor --chip esp32s3 --no-stub --non-interactive --port "$monitor_port" \
    >/dev/null 2>&1 & pid=$!
  sleep 2
  kill "$pid" 2>/dev/null || true
  echo "Flashed and reset via $monitor_port."
fi
