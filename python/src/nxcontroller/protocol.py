"""Wire protocol encoding/decoding. Mirrors protocol/PROTOCOL.md."""

from __future__ import annotations

import struct
from dataclasses import dataclass

PROTOCOL_VERSION = 1
TCP_PORT = 7700
DISCOVERY_PORT = 7701

DISCOVER_REQUEST = b"NXC?"
DISCOVER_REPLY_MAGIC = b"NXC!"

# Client -> device
MSG_HELLO = 0x01
MSG_PING = 0x02
MSG_SET_MODE = 0x03
MSG_SET_WIFI = 0x04
MSG_SET_OPTION = 0x05
MSG_GAMEPAD = 0x10
MSG_KEYBOARD = 0x11
MSG_TYPE = 0x12
MSG_RELEASE_ALL = 0x1F

# Device -> client
MSG_HELLO_ACK = 0x81
MSG_PONG = 0x82
MSG_MODE_ACK = 0x83
MSG_WIFI_ACK = 0x84
MSG_TYPE_PROGRESS = 0x85
MSG_OPTION_ACK = 0x86
MSG_ERROR = 0x8F

FEATURE_GAMEPAD = 0x01
FEATURE_KEYBOARD = 0x02
FEATURE_WIFI = 0x40  # firmware built with WiFi (SET_WIFI works)
FEATURE_PACED_TYPING = 0x80  # device paces TYPE keystrokes itself
MODE_MASK = FEATURE_GAMEPAD | FEATURE_KEYBOARD

# Keystrokes per TYPE message: (255 - 3) // 2.
MAX_TYPE_KEYS = 126
# TYPE flags
TYPE_ROLLOVER = 0x01  # go straight to the next key when no release is needed

# SET_OPTION ids
OPTION_KEYBOARD_POLL_MS = 1

# USB modes. The values are the feature bits the device ends up with.
MODES = {"gamepad": 0x01, "keyboard": 0x02, "both": 0x03}
MODE_NAMES = {v: k for k, v in MODES.items()}

ERROR_NAMES = {
    1: "unknown message type",
    2: "bad payload length",
    3: "not available in the current USB mode",
    4: "protocol version mismatch",
    5: "invalid value",
    6: "failed to save setting on the device",
    7: "device keystroke buffer full",
}


def frame(msg_type: int, payload: bytes = b"") -> bytes:
    if len(payload) > 255:
        raise ValueError("payload too long")
    return bytes((msg_type, len(payload))) + payload


def encode_hello() -> bytes:
    return frame(MSG_HELLO, bytes((PROTOCOL_VERSION,)))


def encode_ping(seq: int) -> bytes:
    return frame(MSG_PING, struct.pack("<I", seq & 0xFFFFFFFF))


def encode_set_mode(mode: int) -> bytes:
    return frame(MSG_SET_MODE, bytes((mode,)))


def encode_set_wifi(ssid: str, password: str) -> bytes:
    s, pw = ssid.encode(), password.encode()
    if not 1 <= len(s) <= 32:
        raise ValueError("WiFi network name must be 1-32 bytes")
    if pw and not 8 <= len(pw) <= 64:
        raise ValueError("WiFi password must be 8-64 bytes (or empty for an open network)")
    return frame(MSG_SET_WIFI, bytes((len(s),)) + s + pw)


def encode_gamepad(buttons: int, hat: int, lx: int, ly: int, rx: int, ry: int) -> bytes:
    return frame(MSG_GAMEPAD, struct.pack("<HBBBBB", buttons, hat, lx, ly, rx, ry))


def encode_keyboard(modifiers: int, keys: list[int] | tuple[int, ...]) -> bytes:
    if len(keys) > 6:
        raise ValueError("at most 6 simultaneous keys")
    padded = list(keys) + [0] * (6 - len(keys))
    return frame(MSG_KEYBOARD, bytes([modifiers, *padded]))


def encode_type(flags: int, press_ms: int, release_ms: int, keys: list[tuple[int, int]]) -> bytes:
    """Keystrokes as (modifiers, key) pairs for the device to type at its own pace."""
    if len(keys) > MAX_TYPE_KEYS:
        raise ValueError(f"at most {MAX_TYPE_KEYS} keys per TYPE message")
    body = bytearray((flags, press_ms, release_ms))
    for modifiers, key in keys:
        body += bytes((modifiers, key))
    return frame(MSG_TYPE, bytes(body))


def encode_set_option(option: int, value: int) -> bytes:
    return frame(MSG_SET_OPTION, bytes((option, value)))


def encode_release_all() -> bytes:
    return frame(MSG_RELEASE_ALL)


@dataclass(frozen=True)
class HelloAck:
    proto_version: int
    features: int
    name: str

    @property
    def has_gamepad(self) -> bool:
        return bool(self.features & FEATURE_GAMEPAD)

    @property
    def has_keyboard(self) -> bool:
        return bool(self.features & FEATURE_KEYBOARD)

    @property
    def mode(self) -> str:
        mode = self.features & MODE_MASK
        return MODE_NAMES.get(mode, f"unknown ({mode})")

    @property
    def has_wifi(self) -> bool:
        return bool(self.features & FEATURE_WIFI)

    @property
    def has_paced_typing(self) -> bool:
        return bool(self.features & FEATURE_PACED_TYPING)


@dataclass(frozen=True)
class Pong:
    seq: int
    usb_configured: bool


@dataclass(frozen=True)
class ModeAck:
    mode: int
    restarting: bool


@dataclass(frozen=True)
class WifiAck:
    pass


@dataclass(frozen=True)
class OptionAck:
    option: int
    value: int
    restarting: bool


@dataclass(frozen=True)
class TypeProgress:
    count: int  # keystrokes finished since the previous TYPE_PROGRESS


@dataclass(frozen=True)
class DeviceError:
    code: int

    @property
    def description(self) -> str:
        return ERROR_NAMES.get(self.code, f"error {self.code}")


Message = HelloAck | Pong | ModeAck | WifiAck | OptionAck | TypeProgress | DeviceError


def decode(msg_type: int, payload: bytes) -> Message:
    if msg_type == MSG_HELLO_ACK and len(payload) >= 2:
        return HelloAck(payload[0], payload[1], payload[2:].decode("utf-8", "replace"))
    if msg_type == MSG_PONG and len(payload) == 5:
        seq, usb = struct.unpack("<IB", payload)
        return Pong(seq, bool(usb))
    if msg_type == MSG_MODE_ACK and len(payload) == 2:
        return ModeAck(payload[0], bool(payload[1]))
    if msg_type == MSG_WIFI_ACK and not payload:
        return WifiAck()
    if msg_type == MSG_OPTION_ACK and len(payload) == 3:
        return OptionAck(payload[0], payload[1], bool(payload[2]))
    if msg_type == MSG_TYPE_PROGRESS and len(payload) == 2:
        return TypeProgress(struct.unpack("<H", payload)[0])
    if msg_type == MSG_ERROR and len(payload) == 1:
        return DeviceError(payload[0])
    raise ValueError(f"unexpected message 0x{msg_type:02x} ({len(payload)} bytes)")
