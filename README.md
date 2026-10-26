# esp-nxcontroller

An ESP32-S3, Raspberry Pi Pico W or Pico 2 W shows up over USB as a
**Switch-compatible wired controller plus a USB keyboard**. You control it from a
web page over Bluetooth, or (ESP32-S3) from a Python library over WiFi.

```
 browser ──Bluetooth LE──┐   ESP32-S3
 (web/)                  ├─▶ Pico W / Pico 2 W ──USB HID──▶ Nintendo Switch (or a PC)
 Python ──WiFi/TCP───────┘   (firmware/)
 (python/)  (ESP32-S3 only)
```

**Start here:** open the project's GitHub Pages site in Chrome or Edge. It
installs the firmware and then works as the remote: gamepad, typing text files,
mode and WiFi settings, and the board's log. To build and run that page locally
with the firmware, use `mise run web:serve` (http://localhost:8700). To work on
the page itself, `cd web && pnpm install && pnpm dev` (no firmware needed; the
page still connects to a board over Bluetooth).

## Layout

| Path        | What                                                  | Toolchain                     |
|-------------|-------------------------------------------------------|-------------------------------|
| `firmware/core/` | Rust `no_std` library shared by every board: protocol, USB HID, BLE, TCP, settings, logs (embassy, trouble-host) | stable Rust (workspace with `pico/`) |
| `firmware/esp32s3/` | ESP32-S3 board (esp-hal, esp-radio)            | Xtensa Rust via `espup`       |
| `firmware/pico/` | Pico W (RP2040) and Pico 2 W (RP2350) board, Bluetooth only (embassy-rp, cyw43) | stable Rust |
| `python/`   | `nxcontroller` package and `nxc` CLI                  | Python ≥3.10, `uv`            |
| `web/`      | The single page: installer (ESP Web Tools) + remote (Web Bluetooth). Preact + Vite + TypeScript | Node + `pnpm` (`mise run web` builds it) |
| `protocol/` | `PROTOCOL.md`, the wire contract between them         | –                             |
| `mise.toml` | Every task (`mise tasks` lists them) and the tools: Node, pnpm, uv, espup, espflash, picotool | [mise](https://mise.jdx.dev) |
| `.mise/tasks/` | The two tasks that need real code: `flash.py` (download mode, ports) and `web/esp32s3.py` (installer images) | uv scripts (uv picks the Python) |

`firmware/` and `python/` share no code and no build system. Each builds,
versions and ships on its own. They are coupled only through
`protocol/PROTOCOL.md`. When you change the protocol, update
`firmware/core/src/protocol.rs` and `python/src/nxcontroller/protocol.py` together.

## Quick start

**No toolchain:** use the web page (above).

**From source** (needs [mise](https://mise.jdx.dev) and [rustup](https://rustup.rs); on
Windows also Git for Windows, with `mise settings set windows_default_inline_shell_args "bash -c"`
so the tasks run in Git Bash):

```bash
mise run setup                   # one-time: Xtensa toolchain (via espup), Python env, firmware/.env
$EDITOR firmware/.env            # optional WIFI_SSID / WIFI_PASSWORD, NXC_NAME
mise run flash                   # ESP32-S3: build + flash + serial monitor
```

The serial log prints the device IP once WiFi is up. Then:

```bash
cd python
uv run nxc discover
uv run nxc ping                  # latency + whether the Switch has enumerated us
uv run nxc tap A
uv run nxc type "hello"
```

mise installs the tools pinned in `mise.toml` the first time a task needs them.
`mise tasks` lists the rest: `test`, `lint`, `build:esp32s3`, `build:pico`,
`monitor`, `mock`, `web`, `web:serve`, ... Extra arguments go through after `--`,
e.g. `mise run flash -- --no-monitor`.

### Raspberry Pi Pico W / Pico 2 W

```bash
mise run build:pico              # both boards -> firmware/pico/dist/*.uf2 (no Xtensa toolchain needed)
mise run build:pico:rp2350       # just one (rp2040: Pico W, rp2350: Pico 2 W)
```

Hold **BOOTSEL** while plugging the Pico in, then copy the `.uf2` for your board
onto the drive that appears (`RPI-RP2` on a Pico W, `RP2350` on a Pico 2 W). The
Pico restarts into the firmware. It has one USB port, which becomes the
gamepad/keyboard, and no serial console: read its log from the web page (Setup →
Logs). The Pico firmware is Bluetooth only and stores its settings in the last
4 KiB flash sector. `firmware/` is a Cargo workspace of `core/` and `pico/` (one
lockfile and target directory; `esp32s3/` stays outside with its own toolchain).
Its Embassy crates are pinned to one git commit (see `firmware/Cargo.toml`), the
one where `cyw43`'s Bluetooth matches trouble-host 0.7, and the CYW43439 radio firmware is vendored in
`firmware/pico/cyw43-firmware/` (Infineon permissive binary license).

### Wiring and flashing (ESP32-S3)

ESP32-S3 DevKitC-style boards have two USB-C ports. Plug **both** into your
computer while developing:

- **USB** (native, GPIO19/20): flashing goes through this port, with the chip's ROM in
  download mode. While the firmware runs, this port is the gamepad/keyboard, so
  `mise run flash` asks you to **hold BOOT, tap RST, release BOOT** before it
  flashes.
- **UART** (CH343/CP210x bridge): carries the log. After flashing, the script resets the board
  through this port and opens the monitor. Flashing *through* the UART port fails on
  macOS with the CH343 chip and the built-in driver: large writes error out.

When you're done, move the native USB cable to the Switch dock. With only the
native port, you flash the same way (BOOT + RST) and press RST afterwards, but
you get no log.

## Python API

```python
from nxcontroller import Controller, Button, Hat, Key, Modifier

with Controller.discover_one() as nx:      # or Controller.connect("192.168.1.50")
    nx.tap("A")                            # press + release
    nx.tap("L", "R")                       # chord
    nx.press(Button.ZL); nx.release("ZL")  # hold / let go
    nx.dpad_tap("up")
    nx.left_stick(0.0, 1.0)                # -1..1, +y is up
    nx.set_gamepad(buttons=["B"], left=(1, 0))
    nx.release_all()

    nx.type_text("Hello, world!\n")        # USB keyboard, US layout; paced by the board
    nx.key_tap("a", modifiers=Modifier.LCTRL)
    print(nx.ping())                       # (latency_s, usb_configured)
```

If the TCP connection drops for any reason, the firmware releases every input,
so a crashed script can't leave a button held down.

### Developing without hardware

```bash
mise run mock                               # fake device on 127.0.0.1:7700 (--keyboard-only / --gamepad-only: uv run nxc mock-device)
cd python && uv run nxc --host 127.0.0.1 ping
```

`mise run test` runs the Python and web tests and `mise run lint` (clippy on the core and every board) in parallel. `mise run lint:core` and `lint:pico` need no Xtensa toolchain.

## Firmware notes

- **WiFi credentials** come from, in order: those saved on the board, then any
  compiled in from `firmware/.env` (gitignored; build through mise), then
  none. With none, or after 3 failed connection attempts, the board opens the open
  `nxcontroller-setup` hotspot with a setup page at `http://192.168.4.1`. It keeps
  retrying its network in the meantime, so a slow router still gets picked up.
  Move it to another network with `uv run nxc wifi "SSID" "password"`.
  The hotspot is open, so anyone nearby can configure the board while it's up.
- **Web page** (`mise run web`, project in `web/`): for the ESP32-S3, ESP Web Tools with the
  bootloader, partition table and app shipped as separate parts, so updating
  doesn't wipe saved WiFi or mode unless you tick "erase"; for the Picos, a `.uf2`
  download with BOOTSEL instructions. It always builds the Bluetooth-only firmware,
  ignoring your `.env`. `web:esp32s3` and `web:pico` write the
  images, `.uf2` files and `manifest.json` into `web/public/` (gitignored), then
  `web:page` runs `pnpm build` to produce `web/dist`, which is what gets hosted. In `web/`: `pnpm dev` (live reload; the install button needs the images in
  `public/`), `pnpm typecheck`, `pnpm test`. CI (`.github/workflows/build.yml`) runs the same tasks on every
  push (lint, and each board's firmware in a job of its own), deploys the page to GitHub Pages from the default branch, and attaches the firmware
  (ESP32-S3 parts plus one merged image for offset 0x0, and both `.uf2` files) to
  releases for `v*` tags.
- **Transports are Cargo features:** `ble` (the web page), `wifi` (the Python
  package; includes the setup hotspot), and `coex` (required when both are on).
  Source builds default to all three. The web page's firmware is Bluetooth only
  (`--no-default-features --features ble`), so it never opens a hotspot or an
  unauthenticated TCP port. BLE pairs without bonding: the link is encrypted, and
  each connection pairs afresh. See `docs/ble-findings.md`.
- **USB mode** is chosen at runtime over WiFi and saved on the board:

  | Mode       | USB device                     | Identity                    |
  |------------|--------------------------------|-----------------------------|
  | `both`     | gamepad + keyboard (default)   | HORI Pokken `0F0D:0092`     |
  | `gamepad`  | gamepad                        | HORI Pokken `0F0D:0092`     |
  | `keyboard` | keyboard                       | pid.codes test `1209:0001`  |

  ```bash
  uv run nxc mode keyboard   # board restarts as a keyboard (~3-5 s), the client reconnects
  ```
  ```python
  nx.set_mode("gamepad")
  ```
  The HORI identity is the standard way to get a Switch to accept a custom wired
  pad without a handshake. Keyboard mode is a plain HID keyboard. `both` works on a
  Switch 2. Use a single mode only if a host rejects the composite device.
  On a Switch, register the pad by pressing L+R on *Controllers → Change Grip/Order*
  (`nxc tap L R --duration 0.5`). Calling an input type the current mode
  lacks raises `NXError`.
- **Typing speed:** the board paces keystrokes itself and never drops one.
  `type_text` uses rollover by default (no release report between different keys).
  On a Mac that measured about 164 chars/s at the default 5 ms keyboard polling and
  about 356 chars/s at 1 ms (`nxc keyboard-poll 1`; `nxc keyboard-poll 0` restores 5 ms).
  `examples/typing_benchmark.py` measures it on your setup.
- WiFi power saving is disabled for latency.
- Log level: set `NXC_LOG` (or `ESP_LOG`) when building, e.g. `NXC_LOG=debug`, or `info,nxcontroller_core::usb=debug`.
- Toolchains: `firmware/esp32s3/rust-toolchain.toml` pins the `esp` channel (run
  `espup update` to upgrade it); `firmware/rust-toolchain.toml` pins the stable
  Rust used by the core and the Picos. Node, pnpm, uv, espup, espflash and
  picotool are pinned in `mise.toml` (the last three from their GitHub releases).
