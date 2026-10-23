# Thin wrappers around scripts/ so `make <target>` works from the repo root.
.PHONY: setup build flash flash-gamepad flash-keyboard monitor test mock

setup:          ; scripts/setup.sh
build:          ; scripts/build.sh
flash:          ; scripts/flash.sh
flash-gamepad:  ; scripts/flash.sh --gamepad-only
flash-keyboard: ; scripts/flash.sh --keyboard-only
monitor:        ; scripts/monitor.sh
test:           ; scripts/test.sh
mock:           ; cd python && uv run nxc mock-device --port 7700
