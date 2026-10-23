"""High-level client for the nxcontroller firmware."""

from __future__ import annotations

import queue
import socket
import threading
import time
from typing import Iterable

from . import protocol as p
from .inputs import (
    STICK_CENTER,
    Button,
    Hat,
    Key,
    Modifier,
    char_to_key,
    parse_button,
    parse_hat,
    parse_key,
)

ButtonLike = Button | str
KeyLike = Key | str

DEFAULT_TAP = 0.1  # seconds held for a tap
DEFAULT_GAP = 0.05  # seconds released after a tap


class NXError(Exception):
    """Raised when the device reports an error or the connection fails."""


def _axis(value: float) -> int:
    """Map -1.0..1.0 onto 0..255 with 0.0 at center."""
    value = max(-1.0, min(1.0, value))
    return max(0, min(255, round(STICK_CENTER + value * 127.5)))


class Controller:
    """A remote controller/keyboard backed by an ESP32-S3 on the network.

    Usage::

        with Controller.connect("192.168.1.50") as nx:
            nx.tap("A")
            nx.left_stick(0, 1.0)   # push up
            nx.type_text("hello")
    """

    def __init__(self, sock: socket.socket):
        self._address: tuple[str, int] = sock.getpeername()[:2]
        self._send_lock = threading.Lock()
        self._waiting = False
        self._seq = 0
        self._reset_state()
        self._attach(sock)

    def _reset_state(self) -> None:
        self._buttons = Button(0)
        self._hat = Hat.CENTER
        self._sticks = [STICK_CENTER] * 4  # lx, ly, rx, ry
        self._modifiers = Modifier(0)
        self._keys: list[int] = []

    def _attach(self, sock: socket.socket) -> None:
        """Start using `sock` as the device connection and say hello."""
        sock.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)
        self._sock = sock
        self._replies: queue.Queue[p.Message] = queue.Queue()
        self._async_error: p.DeviceError | None = None
        self._closed = False
        self._reader = threading.Thread(target=self._read_loop, args=(sock, self._replies), daemon=True)
        self._reader.start()
        self.info: p.HelloAck = self._request(p.encode_hello(), p.HelloAck)

    # ------------------------------------------------------------------ setup

    @classmethod
    def connect(cls, host: str, port: int = p.TCP_PORT, timeout: float = 3.0) -> Controller:
        sock = socket.create_connection((host, port), timeout=timeout)
        sock.settimeout(None)
        return cls(sock)

    @classmethod
    def discover_one(cls, timeout: float = 2.0) -> Controller:
        """Connect to the first device that answers a discovery broadcast."""
        from .discovery import discover

        devices = discover(timeout=timeout)
        if not devices:
            raise NXError("no nxcontroller devices found on the network")
        return cls.connect(devices[0].host, devices[0].port)

    def close(self) -> None:
        if self._closed:
            return
        try:
            self.release_all()
        except (OSError, NXError):
            pass
        self._drop_socket()

    def _drop_socket(self) -> None:
        self._closed = True
        try:
            self._sock.shutdown(socket.SHUT_RDWR)
        except OSError:
            pass
        self._sock.close()

    def __enter__(self) -> Controller:
        return self

    def __exit__(self, *exc: object) -> None:
        self.close()

    # ------------------------------------------------------------- gamepad

    def press(self, *buttons: ButtonLike) -> None:
        """Hold buttons down until `release` is called."""
        for b in buttons:
            self._buttons |= parse_button(b)
        self._send_gamepad()

    def release(self, *buttons: ButtonLike) -> None:
        for b in buttons:
            self._buttons &= ~parse_button(b)
        self._send_gamepad()

    def tap(self, *buttons: ButtonLike, duration: float = DEFAULT_TAP, gap: float = DEFAULT_GAP) -> None:
        """Press buttons together, hold for `duration`, then release."""
        self.press(*buttons)
        time.sleep(duration)
        self.release(*buttons)
        time.sleep(gap)

    def dpad(self, direction: Hat | str) -> None:
        """Hold the D-pad in a direction. Use "CENTER" to let go."""
        self._hat = parse_hat(direction)
        self._send_gamepad()

    def dpad_tap(self, direction: Hat | str, duration: float = DEFAULT_TAP, gap: float = DEFAULT_GAP) -> None:
        self.dpad(direction)
        time.sleep(duration)
        self.dpad(Hat.CENTER)
        time.sleep(gap)

    def left_stick(self, x: float, y: float) -> None:
        """Set the left stick. x/y in -1.0..1.0; +y is up."""
        self._sticks[0], self._sticks[1] = _axis(x), _axis(-y)
        self._send_gamepad()

    def right_stick(self, x: float, y: float) -> None:
        """Set the right stick. x/y in -1.0..1.0; +y is up."""
        self._sticks[2], self._sticks[3] = _axis(x), _axis(-y)
        self._send_gamepad()

    def set_gamepad(
        self,
        buttons: Iterable[ButtonLike] = (),
        hat: Hat | str = Hat.CENTER,
        left: tuple[float, float] = (0.0, 0.0),
        right: tuple[float, float] = (0.0, 0.0),
    ) -> None:
        """Replace the entire gamepad state in one report."""
        self._buttons = Button(0)
        for b in buttons:
            self._buttons |= parse_button(b)
        self._hat = parse_hat(hat)
        self._sticks = [_axis(left[0]), _axis(-left[1]), _axis(right[0]), _axis(-right[1])]
        self._send_gamepad()

    # ------------------------------------------------------------ keyboard

    def key_down(self, *keys: KeyLike, modifiers: Modifier = Modifier(0)) -> None:
        self._modifiers |= modifiers
        for k in keys:
            code = int(parse_key(k))
            if code not in self._keys:
                if len(self._keys) >= 6:
                    raise NXError("at most 6 keys can be held at once")
                self._keys.append(code)
        self._send_keyboard()

    def key_up(self, *keys: KeyLike, modifiers: Modifier = Modifier(0)) -> None:
        self._modifiers &= ~modifiers
        for k in keys:
            code = int(parse_key(k))
            if code in self._keys:
                self._keys.remove(code)
        self._send_keyboard()

    def key_tap(
        self,
        *keys: KeyLike,
        modifiers: Modifier = Modifier(0),
        duration: float = 0.03,
        gap: float = 0.03,
    ) -> None:
        self.key_down(*keys, modifiers=modifiers)
        time.sleep(duration)
        self.key_up(*keys, modifiers=modifiers)
        time.sleep(gap)

    def type_text(self, text: str, interval: float = 0.03) -> None:
        """Type a string using a US keyboard layout."""
        for ch in text:
            mods, key = char_to_key(ch)
            self.key_tap(key, modifiers=mods, duration=interval, gap=interval)

    # -------------------------------------------------------------- misc

    def release_all(self) -> None:
        self._buttons = Button(0)
        self._hat = Hat.CENTER
        self._sticks = [STICK_CENTER] * 4
        self._modifiers = Modifier(0)
        self._keys = []
        self._send(p.encode_release_all())

    def ping(self) -> tuple[float, bool]:
        """Round-trip a ping. Returns (latency_seconds, usb_configured)."""
        self._seq += 1
        start = time.perf_counter()
        pong = self._request(p.encode_ping(self._seq), p.Pong)
        if pong.seq != self._seq:
            raise NXError(f"pong sequence mismatch ({pong.seq} != {self._seq})")
        return time.perf_counter() - start, pong.usb_configured

    @property
    def mode(self) -> str:
        """Current USB mode: "gamepad", "keyboard" or "both"."""
        return self.info.mode

    def set_mode(self, mode: str, timeout: float = 30.0) -> None:
        """Switch what the board presents on USB: "gamepad", "keyboard" or "both".

        The board saves the mode (it survives power cycles) and restarts, which
        takes a few seconds while it rejoins WiFi. This call waits and reconnects.
        The USB host sees the old device unplug and the new one plug in.
        """
        try:
            code = p.MODES[mode.lower()]
        except KeyError:
            raise ValueError(f"unknown mode {mode!r}; choose from {list(p.MODES)}") from None
        ack = self._request(p.encode_set_mode(code), p.ModeAck)
        if not ack.restarting:
            return  # already in that mode

        name = self.info.name
        self._drop_socket()
        self._reset_state()
        time.sleep(1.5)  # give it time to actually go down
        deadline = time.monotonic() + timeout
        while True:
            sock = self._try_reconnect(name)
            if sock is not None:
                self._attach(sock)
                if self.info.features != code:
                    raise NXError(f"device came back in {self.mode!r} mode, expected {mode!r}")
                return
            if time.monotonic() > deadline:
                raise NXError(f"device did not come back within {timeout:.0f}s")
            time.sleep(0.5)

    def _try_reconnect(self, name: str) -> socket.socket | None:
        try:
            sock = socket.create_connection(self._address, timeout=2.0)
        except OSError:
            # DHCP may have handed out a new address; look for it by name.
            from .discovery import discover

            match = next((d for d in discover(timeout=0.5) if d.name == name), None)
            if match is None:
                return None
            try:
                sock = socket.create_connection((match.host, match.port), timeout=2.0)
            except OSError:
                return None
            self._address = (match.host, match.port)
        sock.settimeout(None)
        return sock

    # ----------------------------------------------------------- internals

    def _send_gamepad(self) -> None:
        if not self.info.has_gamepad:
            raise NXError(f"gamepad is not active (device is in {self.mode!r} mode; see set_mode)")
        self._send(p.encode_gamepad(int(self._buttons), int(self._hat), *self._sticks))

    def _send_keyboard(self) -> None:
        if not self.info.has_keyboard:
            raise NXError(f"keyboard is not active (device is in {self.mode!r} mode; see set_mode)")
        self._send(p.encode_keyboard(int(self._modifiers), self._keys))

    def _send(self, data: bytes) -> None:
        if self._closed:
            raise NXError("controller is closed")
        if self._async_error is not None:
            err, self._async_error = self._async_error, None
            raise NXError(f"device error: {err.description}")
        with self._send_lock:
            self._sock.sendall(data)

    def _request(self, data: bytes, expected: type, timeout: float = 2.0):
        self._waiting = True
        try:
            self._send(data)
            reply = self._replies.get(timeout=timeout)
        except queue.Empty:
            raise NXError("timed out waiting for device reply") from None
        finally:
            self._waiting = False
        if isinstance(reply, p.DeviceError):
            raise NXError(f"device error: {reply.description}")
        if not isinstance(reply, expected):
            raise NXError(f"unexpected reply {reply!r}")
        return reply

    def _read_loop(self, sock: socket.socket, replies: queue.Queue[p.Message]) -> None:
        buf = b""
        while True:
            try:
                chunk = sock.recv(256)
            except OSError:
                break
            if not chunk:
                break
            buf += chunk
            while len(buf) >= 2 and len(buf) >= 2 + buf[1]:
                msg_type, length = buf[0], buf[1]
                payload, buf = buf[2 : 2 + length], buf[2 + length :]
                try:
                    msg = p.decode(msg_type, payload)
                except ValueError:
                    continue
                if isinstance(msg, p.DeviceError) and not self._waiting:
                    # Errors for fire-and-forget state messages surface on the next call.
                    self._async_error = msg
                else:
                    replies.put(msg)
