"""Find nxcontroller devices on the local network via UDP broadcast."""

from __future__ import annotations

import socket
import struct
import time
from dataclasses import dataclass

from . import protocol as p


@dataclass(frozen=True)
class DeviceInfo:
    host: str
    port: int
    name: str
    proto_version: int


def discover(timeout: float = 2.0, broadcast: str = "255.255.255.255") -> list[DeviceInfo]:
    """Broadcast a discovery request and collect replies for `timeout` seconds."""
    found: dict[str, DeviceInfo] = {}
    with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as sock:
        sock.setsockopt(socket.SOL_SOCKET, socket.SO_BROADCAST, 1)
        sock.bind(("", 0))
        sock.sendto(p.DISCOVER_REQUEST, (broadcast, p.DISCOVERY_PORT))
        deadline = time.monotonic() + timeout
        while (remaining := deadline - time.monotonic()) > 0:
            sock.settimeout(remaining)
            try:
                data, (host, _) = sock.recvfrom(256)
            except (TimeoutError, socket.timeout):
                break
            if not data.startswith(p.DISCOVER_REPLY_MAGIC) or len(data) < 7:
                continue
            version, port = struct.unpack("<BH", data[4:7])
            name = data[7:].decode("utf-8", "replace")
            found[host] = DeviceInfo(host, port, name, version)
    return list(found.values())
