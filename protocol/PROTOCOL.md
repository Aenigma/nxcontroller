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
| `0x8F` | ERROR      | `code: u8`                                                    |

`features` bit flags: `0x01` gamepad, `0x02` keyboard.

`usb_configured` is `1` once the USB host (Switch or PC) has enumerated the device.

Error codes: `1` unknown message type, `2` bad payload length,
`3` unsupported (for example a keyboard message when the firmware was built
without the keyboard), `4` protocol version mismatch.

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
