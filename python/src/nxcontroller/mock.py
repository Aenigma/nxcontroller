"""A fake device that speaks the protocol, for developing without hardware.

Run it with `nxc mock-device`, then point the client at 127.0.0.1.
"""

from __future__ import annotations

import queue
import socket
import socketserver
import struct
import threading
import time
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
    typed: list[tuple[int, int]] = field(default_factory=list)  # (modifiers, key) from TYPE
    max_outstanding: int = 0  # most keystrokes ever buffered at once


class MockDevice:
    def __init__(
        self,
        host: str = "127.0.0.1",
        port: int = 0,
        gamepad: bool = True,
        keyboard: bool = True,
        verbose: bool = False,
        paced: bool = True,
        key_time: float = 0.0,
    ):
        """`paced`: advertise and handle TYPE. `key_time`: simulated seconds per
        keystroke before TYPE_PROGRESS is sent, to exercise client flow control."""
        self.state = MockState()
        self.gamepad = gamepad
        self.keyboard = keyboard
        self.verbose = verbose
        self.paced = paced
        self.keyboard_poll_ms = 5
        self.key_time = key_time
        self._restart = False
        self._outstanding = 0
        self._send_lock = threading.Lock()
        self._typing: queue.Queue[int | None] = queue.Queue()
        self.wifi: tuple[str, str] | None = None
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
        self._typing = queue.Queue()
        self._outstanding = 0
        threading.Thread(target=self._type_worker, args=(conn, self._typing), daemon=True).start()
        try:
            self._serve_frames(conn)
        finally:
            self._typing.put(None)

    def _type_worker(self, conn: socket.socket, work: queue.Queue[int | None]) -> None:
        """Pretend to type queued keystrokes, then report them like the firmware."""
        while (n := work.get()) is not None:
            time.sleep(self.key_time * n)
            self._outstanding = max(0, self._outstanding - n)
            try:
                with self._send_lock:
                    conn.sendall(p.frame(p.MSG_TYPE_PROGRESS, n.to_bytes(2, "little")))
            except OSError:
                return

    def _serve_frames(self, conn: socket.socket) -> None:
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
                    with self._send_lock:
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
            features |= p.FEATURE_PACED_TYPING if self.paced else 0
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
        if msg_type == p.MSG_SET_WIFI and payload:
            ssid_len = payload[0]
            self.wifi = (payload[1 : 1 + ssid_len].decode(), payload[1 + ssid_len :].decode())
            self._restart = True
            return p.frame(p.MSG_WIFI_ACK)
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
        if msg_type == p.MSG_SET_OPTION and len(payload) == 2:
            if payload[0] != p.OPTION_KEYBOARD_POLL_MS:
                return p.frame(p.MSG_ERROR, b"\x05")
            ms = payload[1] or 5
            if ms == self.keyboard_poll_ms:
                return p.frame(p.MSG_OPTION_ACK, bytes((payload[0], ms, 0)))
            self.keyboard_poll_ms = ms
            self._restart = True
            return p.frame(p.MSG_OPTION_ACK, bytes((payload[0], ms, 1)))
        if msg_type == p.MSG_TYPE and self.paced and len(payload) >= 3 and len(payload) % 2 == 1:
            if not self.keyboard:
                return p.frame(p.MSG_ERROR, b"\x03")
            keys = [(payload[i], payload[i + 1]) for i in range(3, len(payload), 2)]
            if self._outstanding + len(keys) > 512:
                return p.frame(p.MSG_ERROR, b"\x07")
            self._outstanding += len(keys)
            s.max_outstanding = max(s.max_outstanding, self._outstanding)
            s.typed.extend(keys)
            if self.verbose:
                print(f"type {len(keys)} keys (flags {payload[0]:#x}, hold {payload[1]}/{payload[2]} ms)")
            self._typing.put(len(keys))
            return None
        known = {p.MSG_HELLO, p.MSG_PING, p.MSG_SET_MODE, p.MSG_SET_WIFI, p.MSG_SET_OPTION, p.MSG_GAMEPAD, p.MSG_KEYBOARD, p.MSG_RELEASE_ALL}
        return p.frame(p.MSG_ERROR, b"\x02" if msg_type in known else b"\x01")

    def _release_all(self) -> None:
        self.state.buttons, self.state.hat = 0, 8
        self.state.sticks = (128, 128, 128, 128)
        self.state.modifiers, self.state.keys = 0, (0,) * 6
        if self.verbose:
            print("release all")
