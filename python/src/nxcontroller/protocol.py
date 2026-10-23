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
MSG_GAMEPAD = 0x10
MSG_KEYBOARD = 0x11
MSG_RELEASE_ALL = 0x1F

# Device -> client
MSG_HELLO_ACK = 0x81
MSG_PONG = 0x82
MSG_ERROR = 0x8F

FEATURE_GAMEPAD = 0x01
FEATURE_KEYBOARD = 0x02

ERROR_NAMES = {
    1: "unknown message type",
    2: "bad payload length",
    3: "unsupported by this firmware build",
    4: "protocol version mismatch",
}


def frame(msg_type: int, payload: bytes = b"") -> bytes:
    if len(payload) > 255:
        raise ValueError("payload too long")
    return bytes((msg_type, len(payload))) + payload


def encode_hello() -> bytes:
    return frame(MSG_HELLO, bytes((PROTOCOL_VERSION,)))


def encode_ping(seq: int) -> bytes:
    return frame(MSG_PING, struct.pack("<I", seq & 0xFFFFFFFF))


def encode_gamepad(buttons: int, hat: int, lx: int, ly: int, rx: int, ry: int) -> bytes:
    return frame(MSG_GAMEPAD, struct.pack("<HBBBBB", buttons, hat, lx, ly, rx, ry))


def encode_keyboard(modifiers: int, keys: list[int] | tuple[int, ...]) -> bytes:
    if len(keys) > 6:
        raise ValueError("at most 6 simultaneous keys")
    padded = list(keys) + [0] * (6 - len(keys))
    return frame(MSG_KEYBOARD, bytes([modifiers, *padded]))


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


@dataclass(frozen=True)
class Pong:
    seq: int
    usb_configured: bool


@dataclass(frozen=True)
class DeviceError:
    code: int

    @property
    def description(self) -> str:
        return ERROR_NAMES.get(self.code, f"error {self.code}")


Message = HelloAck | Pong | DeviceError


def decode(msg_type: int, payload: bytes) -> Message:
    if msg_type == MSG_HELLO_ACK and len(payload) >= 2:
        return HelloAck(payload[0], payload[1], payload[2:].decode("utf-8", "replace"))
    if msg_type == MSG_PONG and len(payload) == 5:
        seq, usb = struct.unpack("<IB", payload)
        return Pong(seq, bool(usb))
    if msg_type == MSG_ERROR and len(payload) == 1:
        return DeviceError(payload[0])
    raise ValueError(f"unexpected message 0x{msg_type:02x} ({len(payload)} bytes)")
