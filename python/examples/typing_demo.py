"""Typing showcase for the nxcontroller USB keyboard.

Plug the board's native USB port into the computer (or console) you want to type
on, run this script, then click into an empty text editor during the countdown.

    uv run python examples/typing_demo.py                 # auto-discover the board
    uv run python examples/typing_demo.py --host 192.168.1.147 --wpm 120

Everything is typed into whatever window has focus, so pick a blank document.
Ctrl-C stops it at once: closing the connection makes the firmware release all keys.
"""

from __future__ import annotations

import argparse
import random
import time

from nxcontroller import Controller, Key, Modifier
from nxcontroller.inputs import char_to_key


def human_type(nx: Controller, text: str, wpm: float) -> None:
    """Type with per-key jitter and short pauses after punctuation, like a person."""
    base = 60.0 / (wpm * 5)  # seconds per character (a "word" is 5 characters)
    for ch in text:
        mods, key = char_to_key(ch)
        nx.key_tap(key, modifiers=mods, duration=0.02, gap=0)
        delay = random.uniform(0.5, 1.5) * base
        if ch in ".!?\n":
            delay += base * 6
        elif ch in ",;:":
            delay += base * 2
        time.sleep(delay)


def section(nx: Controller, title: str) -> None:
    nx.type_text(f"\n--- {title} ---\n", interval=0.01)
    time.sleep(0.3)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--host", help="device IP (default: auto-discover)")
    parser.add_argument("--wpm", type=float, default=90, help="typing speed for the human-style section")
    parser.add_argument("--countdown", type=int, default=5, help="seconds to focus a text window")
    args = parser.parse_args()

    nx = Controller.connect(args.host) if args.host else Controller.discover_one()
    with nx:
        latency, usb = nx.ping()
        print(f"connected to {nx.info.name!r} ({latency * 1000:.1f} ms)")
        if not nx.info.has_keyboard:
            raise SystemExit("this firmware build has no keyboard (flash without --gamepad-only)")
        if not usb:
            print("warning: the board's native USB port isn't connected to a host yet")

        for i in range(args.countdown, 0, -1):
            print(f"click into an empty text editor... {i}", end="\r", flush=True)
            time.sleep(1)
        print("typing!" + " " * 40)

        # 1. Plain, fast typing: one call, fixed interval.
        section(nx, "type_text: fast and exact")
        nx.type_text("The quick brown fox jumps over the lazy dog. 0123456789\n", interval=0.015)

        # 2. Human-ish rhythm, built from key_tap + char_to_key.
        section(nx, f"human rhythm at ~{args.wpm:.0f} wpm")
        human_type(
            nx,
            "Hello from an ESP32-S3! I'm a USB keyboard, but my keystrokes arrive over WiFi.\n",
            args.wpm,
        )

        # 3. Make a typo, notice it, fix it with backspace.
        section(nx, "typo + backspace")
        human_type(nx, "This is definately", args.wpm)
        time.sleep(0.6)
        for _ in range(5):  # erase "ately"
            nx.key_tap(Key.BACKSPACE, gap=0.08)
        human_type(nx, "itely correct.\n", args.wpm)

        # 4. Select a word with Shift+Left and type over it.
        section(nx, "shift+arrow selection")
        human_type(nx, "My favourite colour is blue", args.wpm)
        time.sleep(0.4)
        for _ in range(4):  # select "blue"
            nx.key_tap(Key.LEFT, modifiers=Modifier.LSHIFT, gap=0.08)
        time.sleep(0.4)
        human_type(nx, "green.\n", args.wpm)

        # 5. Every shifted symbol on a US layout.
        section(nx, "symbols")
        nx.type_text('print("Hello, world!")  # 100% {braces} [brackets] <tags> ~`^&*_+=|\\/?\n', interval=0.02)

        # 6. Holding a modifier across several keys with key_down/key_up.
        section(nx, "held shift")
        nx.key_down(modifiers=Modifier.LSHIFT)
        for letter in "shouting":
            nx.key_tap(letter, gap=0.04)
        nx.key_up(modifiers=Modifier.LSHIFT)
        nx.type_text(" (then back to normal)\n", interval=0.02)

        # 7. Tab and Enter for a small table.
        section(nx, "tab + enter")
        for row in [("button", "press"), ("stick", "tilt"), ("key", "tap")]:
            nx.type_text(row[0], interval=0.02)
            nx.key_tap(Key.TAB)
            nx.type_text(row[1], interval=0.02)
            nx.key_tap(Key.ENTER)

        print("done")


if __name__ == "__main__":
    main()
