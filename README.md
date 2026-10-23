# esp-nxcontroller

An ESP32-S3 joins your WiFi and shows up over its native USB port as a
**Switch-compatible wired controller plus a USB keyboard**. A Python library
drives it remotely.

```
 your PC ──WiFi/TCP──▶ ESP32-S3 ──USB HID──▶ Nintendo Switch (or a PC)
 (python/)              (firmware/)
```

## Layout

| Path        | What                                                  | Toolchain                     |
|-------------|-------------------------------------------------------|-------------------------------|
| `firmware/` | Rust `no_std` firmware (esp-hal, esp-radio, embassy)  | Xtensa Rust via `espup`       |
| `python/`   | `nxcontroller` package and `nxc` CLI                  | Python ≥3.10, `uv`            |
| `protocol/` | `PROTOCOL.md`, the wire contract between the two      | –                             |
| `scripts/`  | setup / build / flash / monitor / test                | bash                          |

`firmware/` and `python/` share no code and no build system. Each builds,
versions and ships on its own. They are coupled only through
`protocol/PROTOCOL.md`. When you change the protocol, update
`firmware/src/protocol.rs` and `python/src/nxcontroller/protocol.py` together.

## Quick start

```bash
scripts/setup.sh                 # one-time: espup + Xtensa toolchain, espflash, python env
$EDITOR firmware/.env            # WIFI_SSID / WIFI_PASSWORD / NXC_NAME
scripts/flash.sh                 # build + flash + serial monitor
```

The serial log prints the device IP once WiFi is up. Then:

```bash
cd python
uv run nxc discover
uv run nxc ping                  # latency + whether the Switch has enumerated us
uv run nxc tap A
uv run nxc type "hello"
```

`make setup|build|flash|monitor|test|mock` wraps the same scripts.

### Wiring and flashing

ESP32-S3 DevKitC-style boards have two USB-C ports. Plug **both** into your
computer while developing:

- **USB** (native, GPIO19/20): flashing goes through this port, with the chip's ROM in
  download mode. While the firmware runs, this port is the gamepad/keyboard, so
  `scripts/flash.sh` asks you to **hold BOOT, tap RST, release BOOT** before it
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

    nx.type_text("Hello, world!\n")        # USB keyboard, US layout
    nx.key_tap("a", modifiers=Modifier.LCTRL)
    print(nx.ping())                       # (latency_s, usb_configured)
```

If the TCP connection drops for any reason, the firmware releases every input,
so a crashed script can't leave a button held down.

### Developing without hardware

```bash
make mock                                   # fake device on 127.0.0.1:7700 (add --keyboard-only / --gamepad-only via nxc mock-device)
cd python && uv run nxc --host 127.0.0.1 ping
```

`scripts/test.sh` runs the Python tests against the mock and runs clippy on the firmware.

## Firmware notes

- WiFi credentials are baked in at compile time from `firmware/.env`, which is
  gitignored. Build through the scripts so the file gets loaded.
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
  pad without a handshake. Keyboard mode is a plain HID keyboard. If your host
  ignores the keyboard half of `both` (a Switch 2 did in our testing), use `keyboard` mode.
  On a Switch, register the pad by pressing L+R on *Controllers → Change Grip/Order*
  (`nxc tap L R --duration 0.5`). Calling an input type the current mode
  lacks raises `NXError`.
- WiFi power saving is disabled for latency.
- Toolchain: `rust-toolchain.toml` pins the `esp` channel. Run `espup update` to upgrade it.
