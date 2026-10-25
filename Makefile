# Thin wrappers around scripts/ so `make <target>` works from the repo root.
.PHONY: setup build flash monitor pico test mock web

setup:          ; scripts/setup.sh
build:          ; scripts/build.sh
flash:          ; scripts/flash.sh
monitor:        ; scripts/monitor.sh
pico:           ; scripts/pico.sh
test:           ; scripts/test.sh
mock:           ; cd python && uv run nxc mock-device --port 7700
web:            ; scripts/web.sh --serve
