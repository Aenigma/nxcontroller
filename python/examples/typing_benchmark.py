#!/usr/bin/env -S uv run --script
"""Measure how fast the board can type into this computer without mistakes.

Plug the board's native USB port into THIS computer, put it in keyboard (or
both) mode, then run this in a terminal and leave that terminal focused:

    uv run python examples/typing_benchmark.py
    uv run python examples/typing_benchmark.py --length 1000 --intervals 0 0.001 0.002 0.005

For each hold time it types a random string. The keystrokes land in this
terminal, and the script reads them back and compares. Ctrl-C stops it.
"""

from __future__ import annotations

import argparse
import difflib
import os
import random
import select
import string
import sys
import termios
import time
import tty

from nxcontroller import Controller, NXError

ALPHABET = string.ascii_letters + string.digits + " .,;'-=/[]!@#$%^&*()_+{}:\"<>?~`|\\"


def read_back(expected_len: int, idle_timeout: float) -> str:
    """Read stdin until `expected_len` chars arrive or input goes quiet."""
    got = bytearray()
    fd = sys.stdin.fileno()
    last = time.monotonic()
    while len(got) < expected_len and time.monotonic() - last < idle_timeout:
        ready, _, _ = select.select([fd], [], [], 0.05)
        if ready:
            got += os.read(fd, 4096)
            last = time.monotonic()
    return got.decode("utf-8", "replace")


def wait_for_focus(nx: Controller) -> bool:
    """Type a marker and check it arrives here, i.e. this terminal has focus."""
    marker = "zq9"
    drain()
    nx.type_text(marker)
    got = read_back(len(marker), idle_timeout=1.0)
    drain()
    return got == marker


def describe_diff(expected: str, got: str, limit: int = 4) -> list[str]:
    sm = difflib.SequenceMatcher(None, expected, got, autojunk=False)
    lines = []
    for op, i1, i2, j1, j2 in sm.get_opcodes():
        if op == "equal":
            continue
        ctx = expected[max(0, i1 - 6) : i1]
        lines.append(f"    at {i1}: {op:7} expected {expected[i1:i2]!r} got {got[j1:j2]!r}  (after ...{ctx!r})")
        if len(lines) == limit:
            break
    return lines


def drain() -> None:
    fd = sys.stdin.fileno()
    while select.select([fd], [], [], 0.2)[0]:
        os.read(fd, 4096)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--host", help="device IP (default: auto-discover)")
    parser.add_argument("--length", type=int, default=500, help="characters per run")
    parser.add_argument("--intervals", type=float, nargs="+", default=[0.0, 0.002, 0.005, 0.01],
                        help="minimum hold times (seconds) to try")
    parser.add_argument("--seed", type=int, default=1)
    args = parser.parse_args()

    if not sys.stdin.isatty():
        raise SystemExit("run this in a terminal (stdin must be a TTY)")

    nx = Controller.connect(args.host) if args.host else Controller.discover_one()
    with nx:
        _, usb = nx.ping()
        if not nx.info.has_keyboard:
            raise SystemExit(f"keyboard not active (mode {nx.mode!r}); run `nxc mode keyboard`")
        if not usb:
            raise SystemExit("the board's native USB port isn't connected to a host")
        print(f"connected to {nx.info.name!r}, paced typing: {nx.info.has_paced_typing}")
        print("keep this terminal focused; the board types into it.\n")

        old = termios.tcgetattr(sys.stdin)
        tty.setcbreak(sys.stdin)  # no line buffering, no echo, Ctrl-C still works
        rng = random.Random(args.seed)
        try:
            time.sleep(1.0)
            drain()
            print(f"{'interval':>9}  {'chars/s':>8}  {'errors':>7}  result")
            for interval in args.intervals:
                while not wait_for_focus(nx):
                    print("  this terminal doesn't have focus: click into it (retrying in 3 s)")
                    time.sleep(3)
                text = "".join(rng.choice(ALPHABET) for _ in range(args.length))
                start = time.monotonic()
                nx.type_text(text, interval=interval)
                typed_s = time.monotonic() - start
                got = read_back(len(text), idle_timeout=2.0)
                drain()
                details: list[str] = []
                if got == text:
                    errors, verdict = 0, "ok"
                elif len(got) < len(text) and text.startswith(got):
                    errors, verdict = 0, f"FOCUS LOST after {len(got)}/{len(text)} chars (not a typing error)"
                else:
                    sm = difflib.SequenceMatcher(None, text, got, autojunk=False)
                    errors = sum(max(i2 - i1, j2 - j1) for op, i1, i2, j1, j2 in sm.get_opcodes() if op != "equal")
                    verdict = f"MISMATCH (got {len(got)}/{len(text)} chars)"
                    details = describe_diff(text, got)
                print(f"{interval * 1000:7.1f}ms  {len(text) / typed_s:8.0f}  {errors:7d}  {verdict}")
                for line in details:
                    print(line)
        except NXError as e:
            print(f"error: {e}")
        finally:
            termios.tcsetattr(sys.stdin, termios.TCSADRAIN, old)


if __name__ == "__main__":
    main()
