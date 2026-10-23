# nxcontroller (Python)

Client library and `nxc` CLI for the nxcontroller ESP32-S3 firmware.

```python
from nxcontroller import Controller

with Controller.discover_one() as nx:       # or Controller.connect("192.168.1.50")
    nx.tap("A")
    nx.press("ZL"); nx.left_stick(0, 1.0)   # hold ZL, push left stick up
    nx.release_all()
    nx.type_text("hello\n")                 # USB keyboard
```

To develop without hardware, run `uv run nxc mock-device --port 7700` in one terminal and
`uv run nxc --host 127.0.0.1 ping` in another.

## Examples

- `examples/typing_demo.py` shows off the keyboard: fast exact typing, human-like rhythm,
  a typo fixed with backspace, Shift+arrow selection, symbols, a held modifier, and Tab/Enter.
  Run `uv run python examples/typing_demo.py` and click into an empty text editor
  during the countdown.
- `examples/type_file.py` types a file or stdin through the keyboard:
  `uv run python examples/type_file.py notes.txt` or `pbpaste | uv run python examples/type_file.py`.
  Smart quotes and dashes become ASCII. Characters a US keyboard can't type are skipped
  (or rejected with `--on-unknown fail`). `--strip-indent` helps in editors that auto-indent.
