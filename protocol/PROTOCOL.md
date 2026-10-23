# nxcontroller wire protocol (v1)

This file is the **contract** between `firmware/` and `python/`. The two sides
share no code, so if you change anything here, update both:

- firmware: `firmware/src/protocol.rs`
- python:   `python/src/nxcontroller/protocol.py`

Bump `PROTOCOL_VERSION` on any incompatible change.

## Transport

| Purpose   | Transport | Port |
|-----------|-----------|------|
| Control   | TCP       | 7700 |
| Discovery | UDP       | 7701 |

The device accepts **one control client at a time**. When that client
disconnects, the device **releases all inputs**, so a crashed script never
leaves a button held down.

All multi-byte integers are little-endian.

## Framing (TCP)

Every message, in both directions, has the same shape:

```
+---------+---------+------------------+
| type u8 | len u8  | payload[len]     |
+---------+---------+------------------+
```

## Client → device

| Type   | Name         | Payload                                                  |
|--------|--------------|----------------------------------------------------------|
| `0x01` | HELLO        | `proto_version: u8`                                      |
| `0x02` | PING         | `seq: u32`                                               |
| `0x03` | SET_MODE     | `mode: u8` (`1` gamepad, `2` keyboard, `3` both)         |
| `0x04` | SET_WIFI     | `ssid_len: u8, ssid: utf8[ssid_len], password: utf8[..]` |
| `0x10` | GAMEPAD      | `buttons: u16, hat: u8, lx: u8, ly: u8, rx: u8, ry: u8`  |
| `0x11` | KEYBOARD     | `modifiers: u8, keys: [u8; 6]`                           |
| `0x1F` | RELEASE_ALL  | *(empty)*                                                |

`GAMEPAD` and `KEYBOARD` carry the **complete** state, not deltas. The device
forwards the latest state as a USB HID report. The client keeps track of what
is held.

The device sends no ack for state messages, to keep latency low. Use `PING` to
check that the link is up.

## Device → client

| Type   | Name       | Payload                                                       |
|--------|------------|---------------------------------------------------------------|
| `0x81` | HELLO_ACK  | `proto_version: u8, features: u8, name: utf8[..]`             |
| `0x82` | PONG       | `seq: u32, usb_configured: u8`                                |
| `0x83` | MODE_ACK   | `mode: u8, restarting: u8`                                    |
| `0x84` | WIFI_ACK   | *(empty)*                                                     |
| `0x8F` | ERROR      | `code: u8`                                                    |

`features` bit flags: `0x01` gamepad, `0x02` keyboard. These reflect the
current USB mode, and mode values use the same bits.

`usb_configured` is `1` once the USB host (Switch or PC) has enumerated the device.

Error codes: `1` unknown message type, `2` bad payload length,
`3` not available in the current USB mode (for example a keyboard message in
gamepad mode), `4` protocol version mismatch, `5` invalid value,
`6` failed to save a setting.

## USB mode

The board presents one of three USB devices:

| Mode | Value | USB device | Identity |
|------|-------|------------|----------|
| gamepad  | `1` | Switch-compatible pad | HORI Pokken `0F0D:0092` |
| keyboard | `2` | boot keyboard         | pid.codes test `1209:0001` |
| both     | `3` | composite pad + keyboard (default) | HORI Pokken `0F0D:0092` |

`SET_MODE` saves the mode to flash, so it survives power cycles. If the
mode actually changes, the device replies `MODE_ACK` with `restarting = 1`,
closes the connection and reboots (a few seconds, including rejoining WiFi). The
USB host sees the old device unplug and the new one plug in. If the requested
mode is already active, it replies `restarting = 0` and nothing happens.

## WiFi setup

`SET_WIFI` saves new credentials (SSID 1–32 bytes, password empty or 8–64
bytes), replies `WIFI_ACK`, closes the connection and reboots onto that network.

A board with no saved credentials, or one that fails to join its network
3 times in a row, opens the open hotspot `nxcontroller-setup` at
`192.168.4.1`. It keeps retrying its network in the meantime. The hotspot serves a
form over HTTP, with DHCP and a catch-all DNS so phones show it as a captive
portal. Saving the form stores the credentials and reboots. Saved credentials take
precedence over any compiled in from `firmware/.env`.

## Gamepad values (Switch / HORIPAD layout)

Buttons (bit flags in `buttons`):

| Bit      | Button  | Bit      | Button  |
|----------|---------|----------|---------|
| `0x0001` | Y       | `0x0100` | MINUS   |
| `0x0002` | B       | `0x0200` | PLUS    |
| `0x0004` | A       | `0x0400` | LSTICK  |
| `0x0008` | X       | `0x0800` | RSTICK  |
| `0x0010` | L       | `0x1000` | HOME    |
| `0x0020` | R       | `0x2000` | CAPTURE |
| `0x0040` | ZL      |          |         |
| `0x0080` | ZR      |          |         |

Hat (D-pad): `0` up, `1` up-right, `2` right, `3` down-right, `4` down,
`5` down-left, `6` left, `7` up-left, `8` centered.

Sticks: `0..=255`, center `128`. On the Y axes, `0` is up.

## Keyboard values

These follow the standard USB HID boot keyboard report. `modifiers` is the bitfield
(`0x01` LCtrl, `0x02` LShift, `0x04` LAlt, `0x08` LGui, `0x10` RCtrl,
`0x20` RShift, `0x40` RAlt, `0x80` RGui). `keys` holds up to 6 HID usage IDs,
padded with `0`.

## Discovery (UDP)

The client broadcasts the 4 ASCII bytes `NXC?` to port 7701. Each device replies
to the sender with:

```
"NXC!" | proto_version: u8 | tcp_port: u16 | name: utf8[..]
```
