#!/usr/bin/env bash
# Build the web page into web/dist (installer + Bluetooth remote): the firmware it
# installs (Bluetooth only, no WiFi) plus an ESP Web Tools manifest go into
# web/public/, then Vite builds the Preact page around them.
#
#   scripts/web.sh            # build into web/dist
#   scripts/web.sh --serve    # build, then serve on http://localhost:8700 (PORT=... to change)
#
# Web Serial only works on https or localhost. To share it, host web/dist on any
# static https host (GitHub Pages, Netlify, ...). To work on the page itself, run
# `pnpm dev` in web/ (needs pnpm and Node; the firmware images are optional there).
set -euo pipefail
. "$(dirname "$0")/_env.sh"

serve=0
[ "${1:-}" = "--serve" ] && serve=1

WEB_DIR="$ROOT_DIR/web"
OUT="$WEB_DIR/public/firmware"
mkdir -p "$OUT"

# Bluetooth only: no setup hotspot, no unauthenticated TCP port. firmware/.env is
# ignored so the published image never carries your WiFi or name.
(cd "$FIRMWARE_DIR" && env -u WIFI_SSID -u WIFI_PASSWORD -u NXC_NAME \
  cargo build --release --no-default-features --features ble)

version="$(sed -n 's/^version *= *"\(.*\)"/\1/p' "$FIRMWARE_DIR/Cargo.toml" | head -1)"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

# A merged image covers the nvs partition and would wipe saved settings on
# every update, so ship the three regions separately instead.
espflash save-image --chip esp32s3 --merge "$ELF" "$tmp/merged.bin" >/dev/null
espflash save-image --chip esp32s3 "$ELF" "$OUT/app.bin" >/dev/null
python3 - "$tmp/merged.bin" "$OUT" <<'EOF'
import sys
merged = open(sys.argv[1], "rb").read()
open(f"{sys.argv[2]}/bootloader.bin", "wb").write(merged[0x0:0x8000])
open(f"{sys.argv[2]}/partitions.bin", "wb").write(merged[0x8000:0x9000])
EOF

cat > "$WEB_DIR/public/manifest.json" <<EOF
{
  "name": "nxcontroller",
  "version": "$version",
  "new_install_prompt_erase": true,
  "builds": [
    {
      "chipFamily": "ESP32-S3",
      "parts": [
        { "path": "firmware/bootloader.bin", "offset": 0 },
        { "path": "firmware/partitions.bin", "offset": 32768 },
        { "path": "firmware/app.bin", "offset": 65536 }
      ]
    }
  ]
}
EOF

(cd "$WEB_DIR" && pnpm install --frozen-lockfile && pnpm build)

echo "Web flasher v$version ready in $WEB_DIR/dist"
if [ "$serve" = 1 ]; then
  port="${PORT:-8700}"
  echo "Open http://localhost:$port in Chrome or Edge (Ctrl-C to stop)"
  cd "$WEB_DIR" && exec pnpm exec vite preview --host 127.0.0.1 --port "$port" --strictPort
fi
