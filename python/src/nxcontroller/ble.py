"""Bluetooth LE transport (skeleton). Needs the optional ``ble`` extra (bleak).

The firmware (built with ``--features ble``) exposes one GATT service that
carries the same ``[type][len][payload]`` frames as TCP port 7700:

- ``TO_DEVICE_UUID``: client -> device, write / write without response. We use
  write (acknowledged): the board may drop unacknowledged writes when its buffers fill.
- ``FROM_DEVICE_UUID``: device -> client, notify.

Both directions are byte streams: a frame can span several writes or
notifications, and one notification can hold several frames. Both
characteristics require an encrypted link, so the OS pairs with the board
(Just Works) the first time; on macOS that happens automatically when we
subscribe.

This module is async (bleak is asyncio-only). It does not plug into
``Controller`` yet; see ``docs/ble-findings.md`` for the plan.

    import asyncio
    from nxcontroller.ble import BleLink

    async def main():
        async with await BleLink.discover_one() as link:
            print(link.info, await link.ping())

    asyncio.run(main())
"""

from __future__ import annotations

import asyncio
import time
from typing import TYPE_CHECKING

from . import protocol as p

if TYPE_CHECKING:  # pragma: no cover
    from bleak import BleakClient
    from bleak.backends.device import BLEDevice

SERVICE_UUID = "a6240001-7a84-4692-ab2b-1fe245e78947"
TO_DEVICE_UUID = "a6240002-7a84-4692-ab2b-1fe245e78947"
FROM_DEVICE_UUID = "a6240003-7a84-4692-ab2b-1fe245e78947"


def _bleak():
    try:
        import bleak
    except ImportError as e:  # pragma: no cover - depends on the environment
        raise ImportError("BLE support needs bleak: pip install 'nxcontroller[ble]'") from e
    return bleak


class FrameBuffer:
    """Reassembles ``[type][len][payload]`` frames from a byte stream."""

    def __init__(self) -> None:
        self._buf = b""

    def feed(self, data: bytes) -> list[tuple[int, bytes]]:
        """Add received bytes; return the frames they completed, in order."""
        self._buf += data
        frames = []
        while len(self._buf) >= 2 and len(self._buf) >= 2 + self._buf[1]:
            msg_type, length = self._buf[0], self._buf[1]
            frames.append((msg_type, self._buf[2 : 2 + length]))
            self._buf = self._buf[2 + length :]
        return frames

    def clear(self) -> None:
        self._buf = b""


def chunks(data: bytes, size: int) -> list[bytes]:
    """Split ``data`` into ATT-sized pieces (the device reassembles them)."""
    if size <= 0:
        raise ValueError("chunk size must be positive")
    return [data[i : i + size] for i in range(0, len(data), size)] or [b""]


async def scan(timeout: float = 5.0) -> list[BLEDevice]:
    """Find boards advertising the nxcontroller service."""
    bleak = _bleak()
    return await bleak.BleakScanner.discover(timeout=timeout, service_uuids=[SERVICE_UUID])


class BleLink:
    """One BLE connection to a board, speaking the frame protocol."""

    def __init__(self, client: BleakClient):
        self._client = client
        self._frames = FrameBuffer()
        self._replies: asyncio.Queue[p.Message] = asyncio.Queue()
        self.info: p.HelloAck | None = None
        self._seq = 0

    @classmethod
    async def connect(cls, device: BLEDevice | str, timeout: float = 10.0) -> BleLink:
        """Connect to ``device`` (a scan result or an address/macOS UUID) and say hello."""
        bleak = _bleak()
        client = bleak.BleakClient(device, timeout=timeout)
        await client.connect()
        link = cls(client)
        try:
            # Subscribing writes the CCCD, which needs encryption: this is where
            # the OS pairs (Just Works) on first use.
            await client.start_notify(FROM_DEVICE_UUID, link._on_notify)
            link.info = await link.request(p.encode_hello(), p.HelloAck)
        except BaseException:
            await client.disconnect()
            raise
        return link

    @classmethod
    async def discover_one(cls, timeout: float = 5.0) -> BleLink:
        devices = await scan(timeout)
        if not devices:
            raise ConnectionError("no nxcontroller found over Bluetooth")
        return await cls.connect(devices[0])

    async def close(self) -> None:
        # The device releases all inputs when the link drops.
        await self._client.disconnect()

    async def __aenter__(self) -> BleLink:
        return self

    async def __aexit__(self, *exc: object) -> None:
        await self.close()

    def _on_notify(self, _characteristic: object, data: bytearray) -> None:
        for msg_type, payload in self._frames.feed(bytes(data)):
            try:
                self._replies.put_nowait(p.decode(msg_type, payload))
            except ValueError:
                continue

    async def send(self, frame: bytes) -> None:
        """Send one frame with acknowledged writes, in pieces of at most ATT_MTU - 3 bytes."""
        for part in chunks(frame, max(20, self._client.mtu_size - 3)):
            await self._client.write_gatt_char(TO_DEVICE_UUID, part, response=True)

    async def request(self, frame: bytes, expected: type, timeout: float = 3.0):
        await self.send(frame)
        reply = await asyncio.wait_for(self._replies.get(), timeout)
        if isinstance(reply, p.DeviceError):
            raise ConnectionError(f"device error: {reply.description}")
        if not isinstance(reply, expected):
            raise ConnectionError(f"unexpected reply {reply!r}")
        return reply

    async def ping(self) -> tuple[float, bool]:
        """Round trip time in seconds, and whether the USB host enumerated the board."""
        self._seq += 1
        start = time.perf_counter()
        pong = await self.request(p.encode_ping(self._seq), p.Pong)
        return time.perf_counter() - start, pong.usb_configured
