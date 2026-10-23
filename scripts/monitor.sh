#!/usr/bin/env bash
# Attach to the serial log via the UART port (resets the board). Pass --port to override.
set -euo pipefail
. "$(dirname "$0")/_env.sh"

args=(--chip esp32s3 --no-stub)
[ -f "$ELF" ] && args+=(--elf "$ELF")   # lets espflash decode backtraces
if [[ " $* " != *" --port "* && " $* " != *" -p "* ]]; then
  port="${ESPFLASH_MONITOR_PORT:-$(find_uart_port)}"
  [ -n "$port" ] && args+=(--port "$port")
fi
exec espflash monitor "${args[@]}" "$@"
