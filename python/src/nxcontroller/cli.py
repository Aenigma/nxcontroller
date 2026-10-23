"""`nxc` command line tool."""

from __future__ import annotations

import argparse
import sys

from . import protocol as p
from .client import Controller, NXError
from .discovery import discover


def _connect(args: argparse.Namespace) -> Controller:
    if args.host:
        return Controller.connect(args.host, args.port)
    return Controller.discover_one()


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(prog="nxc", description="Control an nxcontroller ESP32-S3 over WiFi")
    parser.add_argument("--host", help="device IP (default: auto-discover)")
    parser.add_argument("--port", type=int, default=p.TCP_PORT)
    sub = parser.add_subparsers(dest="cmd", required=True)

    sub.add_parser("discover", help="list devices on the network")
    ping = sub.add_parser("ping", help="measure latency and USB state")
    ping.add_argument("-n", type=int, default=5)
    mode = sub.add_parser("mode", help="show or set the USB mode, e.g. `nxc mode keyboard`")
    mode.add_argument("mode", nargs="?", choices=list(p.MODES))
    wifi = sub.add_parser("wifi", help="move the board to another WiFi network")
    wifi.add_argument("ssid")
    wifi.add_argument("password", nargs="?", default="", help="omit for an open network")
    tap = sub.add_parser("tap", help="tap one or more buttons together, e.g. `nxc tap A`")
    tap.add_argument("buttons", nargs="+")
    tap.add_argument("--duration", type=float, default=0.1)
    dpad = sub.add_parser("dpad", help="tap the D-pad, e.g. `nxc dpad up`")
    dpad.add_argument("direction")
    type_ = sub.add_parser("type", help="type text on the USB keyboard")
    type_.add_argument("text")
    type_.add_argument("--interval", type=float, default=0.0, help="minimum seconds per key state (default: as fast as possible)")
    key = sub.add_parser("key", help="tap a key, e.g. `nxc key enter`")
    key.add_argument("keys", nargs="+")
    mock = sub.add_parser("mock-device", help="run a fake device on localhost for testing")
    mock.add_argument("--bind", default="127.0.0.1")
    only = mock.add_mutually_exclusive_group()
    only.add_argument("--gamepad-only", action="store_true")
    only.add_argument("--keyboard-only", action="store_true")

    args = parser.parse_args(argv)

    try:
        if args.cmd == "discover":
            devices = discover()
            for d in devices:
                print(f"{d.host}:{d.port}  {d.name}  (protocol v{d.proto_version})")
            if not devices:
                print("no devices found", file=sys.stderr)
                return 1
            return 0

        if args.cmd == "mock-device":
            from .mock import MockDevice

            sys.stdout.reconfigure(line_buffering=True)
            dev = MockDevice(
                args.bind,
                args.port,
                gamepad=not args.keyboard_only,
                keyboard=not args.gamepad_only,
                verbose=True,
            )
            print(f"mock device listening on {dev.host}:{dev.port}")
            try:
                dev.serve_forever()
            except KeyboardInterrupt:
                pass
            return 0

        with _connect(args) as nx:
            if args.cmd == "mode":
                if args.mode is None:
                    print(nx.mode)
                elif args.mode == nx.mode:
                    print(f"already in {nx.mode} mode")
                else:
                    print(f"switching {nx.mode} -> {args.mode}, waiting for the board to restart...")
                    nx.set_mode(args.mode)
                    _, usb = nx.ping()
                    print(f"now in {nx.mode} mode (usb {'configured' if usb else 'not connected yet'})")
            elif args.cmd == "wifi":
                nx.set_wifi(args.ssid, args.password)
                print(f"saved; the board is restarting and joining {args.ssid!r}")
                print("if it can't, it opens the 'nxcontroller-setup' hotspot after ~30 s")
            elif args.cmd == "ping":
                print(f"connected to {nx.info.name!r} (protocol v{nx.info.proto_version})")
                for _ in range(args.n):
                    latency, usb = nx.ping()
                    print(f"{latency * 1000:6.1f} ms  usb={'configured' if usb else 'not connected'}")
            elif args.cmd == "tap":
                nx.tap(*args.buttons, duration=args.duration)
            elif args.cmd == "dpad":
                nx.dpad_tap(args.direction)
            elif args.cmd == "type":
                nx.type_text(args.text, interval=args.interval)
            elif args.cmd == "key":
                nx.key_tap(*args.keys)
    except (NXError, OSError, ValueError) as e:
        print(f"error: {e}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
