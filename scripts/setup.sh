#!/usr/bin/env bash
# One-time host setup: Xtensa Rust toolchain (via espup), espflash, and the Python env.
set -euo pipefail
cd "$(dirname "$0")/.."

need() { command -v "$1" >/dev/null 2>&1; }

if ! need cargo; then
  echo "Rust is not installed. Get it from https://rustup.rs and re-run." >&2
  exit 1
fi

echo "==> espup (installs the Xtensa Rust toolchain for the ESP32-S3)"
need espup || cargo install --locked espup

echo "==> espflash (flashing + serial monitor)"
need espflash || cargo install --locked espflash

echo "==> Xtensa toolchain"
if rustup toolchain list | grep -q '^esp'; then
  echo "    'esp' toolchain already installed (run 'espup update' to upgrade)"
else
  espup install --targets esp32s3 --export-file "$HOME/export-esp.sh"
fi

if [ ! -f firmware/.env ]; then
  cp firmware/.env.example firmware/.env
  echo "==> Created firmware/.env. Put your WiFi credentials in it."
fi

if need uv; then
  echo "==> Python env (uv)"
  (cd python && uv sync)
else
  echo "==> uv not found; skipping Python env (pip install -e python/ works too)"
fi

echo
echo "Done. Next: edit firmware/.env, plug in the board, then run scripts/flash.sh"
