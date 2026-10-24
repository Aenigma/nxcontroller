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
| `0x05` | SET_OPTION   | `option: u8, value: u8`                                  |
| `0x12` | TYPE         | `flags: u8, press_ms: u8, release_ms: u8, (modifiers: u8, key: u8)[..126]` |
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
| `0x85` | TYPE_PROGRESS | `count: u16` (keystrokes finished since the last one)      |
| `0x86` | OPTION_ACK | `option: u8, value: u8, restarting: u8`                       |
| `0x8F` | ERROR      | `code: u8`                                                    |

`features` bit flags: `0x01` gamepad, `0x02` keyboard, `0x80` paced typing (TYPE). These reflect the
current USB mode, and mode values use the same bits.

`usb_configured` is `1` once the USB host (Switch or PC) has enumerated the device.

Error codes: `1` unknown message type, `2` bad payload length,
`3` not available in the current USB mode (for example a keyboard message in
gamepad mode), `4` protocol version mismatch, `5` invalid value,
`6` failed to save a setting, `7` keystroke buffer full (TYPE rejected,
nothing queued).

## Paced typing (TYPE)

Firmware that sets feature bit `0x80` in HELLO_ACK types `TYPE` keystrokes at
the pace the USB host reads them, instead of the client timing KEYBOARD states
over the network. For each `(modifiers, key)` it:

1. changes the modifiers on their own first, if they differ from the current state
   (releasing any key still down);
2. presses the key, then holds for at least `press_ms`;
3. releases the key (so repeated keys register twice), then holds for at least `release_ms`;
4. releases everything once the queue is empty.

With flag `0x01` (rollover), step 3 is skipped. The next keystroke, if it also
has the flag, goes straight from the held key to its own in one report when its
modifiers match and the key differs; otherwise it releases first as in step 1.
(The Python client types Enter on its own: a KEYBOARD all-released state, then a
non-rollover TYPE with at least 20 ms holds, because the Switch 2's text field
missed some rolled-over Enters.) That needs about one report
per character instead of two.

Typed text comes out as written regardless of the host's Caps Lock: the device
tracks the host's LED output report, and before a letter while Caps Lock is on, it
holds Caps Lock until the host reports it off (at most 400 ms), then continues. If the
host doesn't confirm, it stops correcting until the queue drains. KEYBOARD states
are never adjusted.

Every report write completes only after the host has taken the report, so no
state is skipped however fast keystrokes are queued. `press_ms`/`release_ms`
(0 = none) add time for apps that only sample the keyboard once per frame.

Flow control: the device buffers up to 512 keyboard commands (TYPE keystrokes
and KEYBOARD states share one ordered queue). It reports finished keystrokes
with `TYPE_PROGRESS`, sent while the client is idle, and coalesced. Clients
should keep at most 256 keystrokes outstanding (sent minus reported). A TYPE that
doesn't fit is rejected whole with error `7`. RELEASE_ALL and disconnecting clear the
queue, so typing stops within about one keystroke.

## Options (SET_OPTION)

| Option | Value | Effect |
|--------|-------|--------|
| `1` keyboard poll interval | `1..=255` ms, `0` = default (5) | How often the host polls the keyboard endpoint. Lower is faster typing. |

Options are saved with the other settings. If the value changes, the device
replies `OPTION_ACK` with `restarting = 1`, then closes the connection and
reboots (the USB device re-enumerates). Otherwise it replies with `restarting = 0`.
An unknown option is error `5`.

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
