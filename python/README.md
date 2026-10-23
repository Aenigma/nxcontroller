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
