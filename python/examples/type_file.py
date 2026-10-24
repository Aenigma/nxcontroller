#!/usr/bin/env -S uv run --script
"""Type a text file (or stdin) out through the nxcontroller USB keyboard.

    uv run python examples/type_file.py notes.txt
    cat notes.txt | uv run python examples/type_file.py
    pbpaste | uv run python examples/type_file.py --host 192.168.1.147 --interval 0.01

Click into the target window during the countdown. Ctrl-C stops typing at once,
and the firmware releases every key when the connection closes.
"""

from __future__ import annotations

import argparse
import sys
import time

from nxcontroller import Controller, NXError
from nxcontroller.inputs import char_to_key

# Common "smart" characters mapped to what a US keyboard can type.
REPLACEMENTS = {
    "‘": "'", "’": "'", "“": '"', "”": '"',
    "–": "-", "—": "--", "…": "...", " ": " ",
}


def typeable(ch: str) -> bool:
    try:
        char_to_key(ch)
        return True
    except ValueError:
        return False


def prepare(text: str, on_unknown: str, strip_indent: bool) -> str:
    text = text.replace("\r\n", "\n").replace("\r", "\n")
    text = "".join(REPLACEMENTS.get(ch, ch) for ch in text)
    if strip_indent:
        text = "\n".join(line.lstrip(" \t") for line in text.split("\n"))

    bad = [
        (lineno, col, ch)
        for lineno, line in enumerate(text.split("\n"), 1)
        for col, ch in enumerate(line, 1)
        if not typeable(ch)
    ]
    if bad and on_unknown == "fail":
        for lineno, col, ch in bad[:10]:
            print(f"  line {lineno}, col {col}: {ch!r} (U+{ord(ch):04X})", file=sys.stderr)
        raise SystemExit(f"{len(bad)} character(s) can't be typed on a US layout (use --on-unknown skip)")
    if bad:
        print(f"skipping {len(bad)} untypeable character(s), e.g. {bad[0][2]!r}", file=sys.stderr)
        text = "".join(ch for ch in text if ch == "\n" or typeable(ch))
    return text


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("file", nargs="?", default="-", help="text file to type (default: stdin)")
    parser.add_argument("--host", help="device IP (default: auto-discover)")
    parser.add_argument("--interval", type=float, default=0.02,
                        help="minimum seconds each key stays pressed and released (max 0.255; default 0.02, "
                             "which a Switch 2 needed). 0 = as fast as the USB host reads")
    parser.add_argument("--countdown", type=int, default=5, help="seconds to focus the target window")
    parser.add_argument("--no-rollover", action="store_true",
                        help="release every key before the next one (slower; try it if a host misreads keys)")
    parser.add_argument("--on-unknown", choices=["skip", "fail"], default="skip",
                        help="what to do with characters a US keyboard can't type")
    parser.add_argument("--strip-indent", action="store_true",
                        help="drop leading whitespace per line, for editors that auto-indent")
    args = parser.parse_args()

    # Read everything up front so a pipe is drained before we start typing.
    if args.file == "-":
        raw = sys.stdin.read()
    else:
        with open(args.file, encoding="utf-8") as f:
            raw = f.read()
    text = prepare(raw, args.on_unknown, args.strip_indent)
    if not text:
        raise SystemExit("nothing to type")

    print(f"{len(text)} characters", file=sys.stderr)

    try:
        nx = Controller.connect(args.host) if args.host else Controller.discover_one()
    except (OSError, NXError) as e:
        raise SystemExit(f"could not connect to the board: {e}") from None
    with nx:
        if not nx.info.has_keyboard:
            raise SystemExit(f"the keyboard isn't active (board is in {nx.mode!r} mode; try `nxc mode keyboard`)")
        if not nx.info.has_paced_typing:
            print("note: old firmware, typing is timed from here (slower; reflash for paced typing)",
                  file=sys.stderr)

        for i in range(args.countdown, 0, -1):
            print(f"focus the target window... {i} ", end="\r", file=sys.stderr, flush=True)
            time.sleep(1)

        start = time.monotonic()
        done = 0

        def progress(n: int, total: int) -> None:
            nonlocal done
            done = n
            print(f"typed {n}/{total} ({n * 100 // total}%)   ", end="\r", file=sys.stderr, flush=True)

        try:
            nx.type_text(text, interval=args.interval, on_progress=progress, rollover=not args.no_rollover)
        except KeyboardInterrupt:
            # Leaving the `with` block releases everything and clears the board's queue.
            print(f"\nstopped after {done} characters", file=sys.stderr)
            raise SystemExit(130) from None
        elapsed = time.monotonic() - start
        print(f"\ndone in {elapsed:.1f}s ({len(text) / elapsed:.0f} chars/s)", file=sys.stderr)

if __name__ == "__main__":
    main()
