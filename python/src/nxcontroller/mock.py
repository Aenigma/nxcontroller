"""A fake device that speaks the protocol, for developing without hardware.

Run it with `nxc mock-device`, then point the client at 127.0.0.1.
"""

from __future__ import annotations

import socket
import socketserver
import struct
import threading
from dataclasses import dataclass, field

from . import protocol as p


@dataclass
class MockState:
    buttons: int = 0
    hat: int = 8
    sticks: tuple[int, int, int, int] = (128, 128, 128, 128)
    modifiers: int = 0
    keys: tuple[int, ...] = (0,) * 6
    log: list[tuple[int, bytes]] = field(default_factory=list)


class MockDevice:
    def __init__(
        self,
        host: str = "127.0.0.1",
        port: int = 0,
        gamepad: bool = True,
        keyboard: bool = True,
        verbose: bool = False,
    ):
        self.state = MockState()
        self.gamepad = gamepad
        self.keyboard = keyboard
        self.verbose = verbose
        self._restart = False
        device = self

        class Handler(socketserver.BaseRequestHandler):
            def handle(self) -> None:
                device._serve(self.request)

        self._server = socketserver.ThreadingTCPServer((host, port), Handler)
        self._server.daemon_threads = True
        self.host, self.port = self._server.server_address[:2]
        self._thread = threading.Thread(target=self._server.serve_forever, daemon=True)

    def start(self) -> MockDevice:
        self._thread.start()
        return self

    def stop(self) -> None:
        self._server.shutdown()
        self._server.server_close()

    def serve_forever(self) -> None:
        self._server.serve_forever()

    def _serve(self, conn: socket.socket) -> None:
        buf = b""
        while True:
            chunk = conn.recv(256)
            if not chunk:
                break
            buf += chunk
            while len(buf) >= 2 and len(buf) >= 2 + buf[1]:
                msg_type, length = buf[0], buf[1]
                payload, buf = buf[2 : 2 + length], buf[2 + length :]
                reply = self._handle(msg_type, payload)
                if reply:
                    conn.sendall(reply)
                if self._restart:  # simulate the reboot after a mode change
                    self._restart = False
                    conn.close()
                    self._release_all()
                    return
        self._release_all()

    def _handle(self, msg_type: int, payload: bytes) -> bytes | None:
        s = self.state
        s.log.append((msg_type, payload))
        if msg_type == p.MSG_HELLO:
            if payload[:1] != bytes((p.PROTOCOL_VERSION,)):
                return p.frame(p.MSG_ERROR, b"\x04")
            features = (p.FEATURE_GAMEPAD if self.gamepad else 0) | (p.FEATURE_KEYBOARD if self.keyboard else 0)
            return p.frame(p.MSG_HELLO_ACK, bytes((p.PROTOCOL_VERSION, features)) + b"mock")
        if msg_type == p.MSG_SET_MODE and len(payload) == 1:
            if payload[0] not in p.MODE_NAMES:
                return p.frame(p.MSG_ERROR, b"\x05")
            new = (payload[0] & 1 == 1, payload[0] & 2 == 2)
            if new == (self.gamepad, self.keyboard):
                return p.frame(p.MSG_MODE_ACK, payload + b"\x00")
            self.gamepad, self.keyboard = new
            self._restart = True
            if self.verbose:
                print(f"mode -> {p.MODE_NAMES[payload[0]]} (restarting)")
            return p.frame(p.MSG_MODE_ACK, payload + b"\x01")
        if msg_type == p.MSG_PING and len(payload) == 4:
            return p.frame(p.MSG_PONG, payload + b"\x01")
        if msg_type == p.MSG_GAMEPAD and len(payload) == 7:
            if not self.gamepad:
                return p.frame(p.MSG_ERROR, b"\x03")
            s.buttons, s.hat, *sticks = struct.unpack("<HBBBBB", payload)
            s.sticks = tuple(sticks)
            if self.verbose:
                print(f"gamepad buttons=0x{s.buttons:04x} hat={s.hat} sticks={s.sticks}")
            return None
        if msg_type == p.MSG_KEYBOARD and len(payload) == 7:
            if not self.keyboard:
                return p.frame(p.MSG_ERROR, b"\x03")
            s.modifiers, s.keys = payload[0], tuple(payload[1:])
            if self.verbose:
                print(f"keyboard mods=0x{s.modifiers:02x} keys={list(s.keys)}")
            return None
        if msg_type == p.MSG_RELEASE_ALL and not payload:
            self._release_all()
            return None
        known = {p.MSG_HELLO, p.MSG_PING, p.MSG_SET_MODE, p.MSG_GAMEPAD, p.MSG_KEYBOARD, p.MSG_RELEASE_ALL}
        return p.frame(p.MSG_ERROR, b"\x02" if msg_type in known else b"\x01")

    def _release_all(self) -> None:
        self.state.buttons, self.state.hat = 0, 8
        self.state.sticks = (128, 128, 128, 128)
        self.state.modifiers, self.state.keys = 0, (0,) * 6
        if self.verbose:
            print("release all")
