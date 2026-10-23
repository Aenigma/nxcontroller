#!/usr/bin/env bash
# Build the firmware (release). Extra args go to cargo, e.g.:
#   scripts/build.sh --no-default-features --features gamepad    # gamepad only
#   scripts/build.sh --no-default-features --features keyboard   # keyboard only
set -euo pipefail
. "$(dirname "$0")/_env.sh"
load_firmware_env

cd "$FIRMWARE_DIR"
cargo build --release "$@"
echo "Built $ELF"
