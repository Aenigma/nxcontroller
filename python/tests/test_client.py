import socket
import threading
import time

import pytest

from nxcontroller import Button, Controller, Hat, Key, Modifier, NXError
from nxcontroller import protocol as p
from nxcontroller.inputs import char_to_key
from nxcontroller.mock import DEFAULT_LOG, MockDevice


@pytest.fixture
def device():
    dev = MockDevice().start()
    yield dev
    dev.stop()


def wait_for(cond, timeout=1.0):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if cond():
            return
        time.sleep(0.005)
    raise AssertionError("condition not met")


def test_encode_gamepad():
    assert p.encode_gamepad(0x0004, 8, 128, 128, 128, 128) == bytes([0x10, 7, 0x04, 0, 8, 128, 128, 128, 128])


def test_encode_keyboard_pads_keys():
    assert p.encode_keyboard(0x02, [0x04]) == bytes([0x11, 7, 0x02, 0x04, 0, 0, 0, 0, 0])


def test_char_to_key():
    assert char_to_key("a") == (Modifier(0), Key.A)
    assert char_to_key("A") == (Modifier.LSHIFT, Key.A)
    assert char_to_key("!") == (Modifier.LSHIFT, Key.N1)
    assert char_to_key("0") == (Modifier(0), Key.N0)


def test_hello_and_ping(device):
    with Controller.connect(device.host, device.port) as nx:
        assert nx.info.name == "mock"
        assert nx.info.has_keyboard
        latency, usb = nx.ping()
        assert usb and latency < 1.0


def test_press_release_and_sticks(device):
    with Controller.connect(device.host, device.port) as nx:
        nx.press("a", Button.ZR)
        wait_for(lambda: device.state.buttons == Button.A | Button.ZR)
        nx.release("A")
        wait_for(lambda: device.state.buttons == Button.ZR)
        nx.dpad("up-left")
        wait_for(lambda: device.state.hat == Hat.UP_LEFT)
        nx.left_stick(0, 1.0)  # up -> Y axis 0
        wait_for(lambda: device.state.sticks[:2] == (128, 0))
        nx.right_stick(1.0, -1.0)
        wait_for(lambda: device.state.sticks[2:] == (255, 255))


def test_disconnect_releases(device):
    nx = Controller.connect(device.host, device.port)
    nx.press("B")
    wait_for(lambda: device.state.buttons == Button.B)
    nx.close()
    wait_for(lambda: device.state.buttons == 0)


def test_keyboard_state(device):
    with Controller.connect(device.host, device.port) as nx:
        nx.key_down("h", modifiers=Modifier.LSHIFT)
        wait_for(lambda: device.state.keys[0] == Key.H and device.state.modifiers == Modifier.LSHIFT)
        nx.key_up("h", modifiers=Modifier.LSHIFT)
        wait_for(lambda: device.state.keys[0] == 0 and device.state.modifiers == 0)


def test_keyboard_unsupported():
    dev = MockDevice(keyboard=False).start()
    try:
        with Controller.connect(dev.host, dev.port) as nx:
            with pytest.raises(NXError):
                nx.key_tap("a")
    finally:
        dev.stop()


def test_gamepad_unsupported_keyboard_only():
    dev = MockDevice(gamepad=False).start()
    try:
        with Controller.connect(dev.host, dev.port) as nx:
            assert not nx.info.has_gamepad
            with pytest.raises(NXError):
                nx.tap("A")
            nx.key_tap("a")
            wait_for(lambda: dev.state.keys[0] == 0)
    finally:
        dev.stop()


def test_set_mode_reconnects(device):
    with Controller.connect(device.host, device.port) as nx:
        assert nx.mode == "both"
        nx.set_mode("keyboard", timeout=5)
        assert nx.mode == "keyboard"
        assert (device.gamepad, device.keyboard) == (False, True)
        with pytest.raises(NXError):
            nx.tap("A")
        nx.key_tap("a")
        nx.set_mode("keyboard")  # no-op, no restart
        nx.set_mode("both", timeout=5)
        nx.tap("A")
        wait_for(lambda: device.state.buttons == 0)


def test_set_mode_rejects_unknown(device):
    with Controller.connect(device.host, device.port) as nx:
        with pytest.raises(ValueError):
            nx.set_mode("mouse")


def test_set_wifi(device):
    nx = Controller.connect(device.host, device.port)
    nx.set_wifi("Home Net", "hunter2hunter2")
    assert device.wifi == ("Home Net", "hunter2hunter2")
    with pytest.raises(NXError):
        nx.ping()  # connection is closed after the board restarts


def test_set_wifi_validates(device):
    with Controller.connect(device.host, device.port) as nx:
        with pytest.raises(ValueError):
            nx.set_wifi("", "whatever1")
        with pytest.raises(ValueError):
            nx.set_wifi("net", "short")


def _decode_typed(keys):
    rev = {}
    for ch in map(chr, range(32, 127)):
        m, k = char_to_key(ch)
        rev[(int(m), int(k))] = ch
    for ch in "\n\t":
        m, k = char_to_key(ch)
        rev[(int(m), int(k))] = ch
    return "".join(rev[mk] for mk in keys)


SAMPLE = "Hello, World! The quick brown fox jumps over the lazy dog. 0123456789 ~!@#$%^&*()_+{}|:\"<>?\n\tdone"


def test_type_text_paced(device):
    with Controller.connect(device.host, device.port) as nx:
        assert nx.info.has_paced_typing
        nx.type_text(SAMPLE, interval=0.004)
    assert _decode_typed(device.state.typed) == SAMPLE
    type_msgs = [pl for t, pl in device.state.log if t == p.MSG_TYPE]
    regular = [pl for pl in type_msgs if pl[4] != int(Key.ENTER)]  # Enter goes alone, see below
    assert regular and all(pl[:3] == bytes((p.TYPE_ROLLOVER, 4, 4)) for pl in regular)  # rollover is the default
    # Paced typing never falls back to per-key KEYBOARD messages; the only ones are
    # the all-released states sent before each Enter.
    kb = [pl for t, pl in device.state.log if t == p.MSG_KEYBOARD]
    assert len(kb) == SAMPLE.count("\n") and all(pl == bytes(7) for pl in kb)


def test_type_text_flow_control_with_slow_device():
    dev = MockDevice(key_time=0.0003).start()  # device types slower than we can send
    text = (SAMPLE * 12)[:1000]
    progress = []
    try:
        with Controller.connect(dev.host, dev.port) as nx:
            nx.type_text(text, on_progress=lambda done, total: progress.append((done, total)))
    finally:
        dev.stop()
    assert _decode_typed(dev.state.typed) == text
    assert 0 < dev.state.max_outstanding <= 256  # never more than the window in flight
    assert progress[-1] == (1000, 1000)
    assert [d for d, _ in progress] == sorted(d for d, _ in progress)


def test_type_text_falls_back_on_old_firmware():
    dev = MockDevice(paced=False).start()
    try:
        with Controller.connect(dev.host, dev.port) as nx:
            assert not nx.info.has_paced_typing
            nx.type_text("hi", interval=0.0)
    finally:
        dev.stop()
    assert dev.state.typed == []
    kb = [pl for t, pl in dev.state.log if t == p.MSG_KEYBOARD]
    assert [pl[1] for pl in kb][:4] == [int(Key.H), 0, int(Key.I), 0]


def test_type_text_without_rollover(device):
    with Controller.connect(device.host, device.port) as nx:
        nx.type_text("abc", rollover=False)
    type_msgs = [pl for t, pl in device.state.log if t == p.MSG_TYPE]
    assert type_msgs and all(pl[0] == 0 for pl in type_msgs)
    assert _decode_typed(device.state.typed) == "abc"


def test_set_keyboard_poll_ms_restarts_and_reconnects(device):
    with Controller.connect(device.host, device.port) as nx:
        nx.set_keyboard_poll_ms(1, timeout=5)
        assert device.keyboard_poll_ms == 1
        nx.set_keyboard_poll_ms(1)  # unchanged: no restart
        nx.type_text("ok")
    assert _decode_typed(device.state.typed) == "ok"


def test_enter_is_typed_alone_after_a_release(device):
    with Controller.connect(device.host, device.port) as nx:
        nx.type_text("ab\ncd\n\nx")
    assert _decode_typed(device.state.typed) == "ab\ncd\n\nx"
    msgs = [(t, pl) for t, pl in device.state.log if t in (p.MSG_TYPE, p.MSG_KEYBOARD)]
    for i, (t, pl) in enumerate(msgs):
        keys = [pl[j + 1] for j in range(3, len(pl), 2)] if t == p.MSG_TYPE else []
        if int(Key.ENTER) in keys:
            assert keys == [int(Key.ENTER)]  # alone in its message
            assert pl[0] == 0 and pl[1] >= 20 and pl[2] >= 20  # no rollover, held longer
            prev_t, prev_pl = msgs[i - 1]
            assert prev_t == p.MSG_KEYBOARD and prev_pl == bytes(7)  # everything released first


def run_with_timeout(fn, timeout=2.0):
    """Call `fn` in a thread and fail the test instead of hanging if it never returns."""
    result: dict = {}

    def target() -> None:
        try:
            result["value"] = fn()
        except BaseException as e:
            result["error"] = e

    t = threading.Thread(target=target, daemon=True)
    t.start()
    t.join(timeout)
    assert not t.is_alive(), "timed out"
    if "error" in result:
        raise result["error"]
    return result["value"]


def read_until(stream, text: str) -> str:
    """Collect followed log text until it ends with `text`."""

    def consume() -> str:
        out = ""
        for chunk in stream:
            out += chunk
            if out.endswith(text):
                return out
        return out

    out = run_with_timeout(consume)
    assert out.endswith(text), out
    return out


def test_encode_logs():
    assert p.encode_logs(False) == bytes([0x06, 1, 0])
    assert p.encode_logs(True) == bytes([0x06, 1, 1])
    assert p.decode(p.MSG_LOG, b"") == p.LogChunk(b"")


def test_logs_buffered(device):
    with Controller.connect(device.host, device.port) as nx:
        assert nx.info.has_logs
        assert nx.logs(timeout=0.2) == DEFAULT_LOG  # 221 bytes: split at 200 mid-line


def test_logs_split_inside_multibyte_character():
    text = "h\u00e9llo w\u00f6rld\n\u2192 second line\n"
    dev = MockDevice(log_text=text, log_chunk=2).start()  # every 2nd byte ends a frame
    try:
        pieces = [text.encode()[i : i + 2] for i in range(0, len(text.encode()), 2)]
        assert any(piece.endswith(b"\xc3") for piece in pieces)  # a split inside "é"
        with Controller.connect(dev.host, dev.port) as nx:
            assert nx.logs(timeout=0.2) == text
    finally:
        dev.stop()


def test_logs_empty_buffer():
    dev = MockDevice(log_text="").start()
    try:
        with Controller.connect(dev.host, dev.port) as nx:
            assert nx.logs(timeout=0.1) == ""
    finally:
        dev.stop()


def test_follow_logs_receives_new_output(device):
    with Controller.connect(device.host, device.port) as nx:
        stream = nx.follow_logs()
        read_until(stream, DEFAULT_LOG)
        device.emit_log("new line from the device\n")
        read_until(stream, "new line from the device\n")
        stream.close()


def test_follow_stop_sends_logs_0(device):
    with Controller.connect(device.host, device.port) as nx:
        stream = nx.follow_logs()
        read_until(stream, DEFAULT_LOG)
        stream.close()
        wait_for(lambda: [pl for t, pl in device.state.log if t == p.MSG_LOGS][-1:] == [b"\x00"])
        assert [pl for t, pl in device.state.log if t == p.MSG_LOGS] == [b"\x01", b"\x00"]
        assert nx.logs(timeout=0.2) == DEFAULT_LOG  # the connection still works


def test_follow_logs_fails_when_connection_drops(device):
    with Controller.connect(device.host, device.port) as nx:
        stream = nx.follow_logs()
        read_until(stream, DEFAULT_LOG)
        for conn in list(device._followers):
            conn.shutdown(socket.SHUT_RDWR)  # the device goes away mid-stream
        with pytest.raises(NXError, match="connection lost"):
            run_with_timeout(lambda: list(stream))


def test_logs_without_feature_raises():
    dev = MockDevice(logs=False).start()
    try:
        with Controller.connect(dev.host, dev.port) as nx:
            assert not nx.info.has_logs
            with pytest.raises(NXError, match="no device log"):
                nx.logs()
            with pytest.raises(NXError, match="no device log"):
                nx.follow_logs()
            assert nx.ping()[1]  # the connection is still usable
    finally:
        dev.stop()


def test_log_frames_do_not_disturb_ping(device):
    with Controller.connect(device.host, device.port) as nx:
        stream = nx.follow_logs()
        read_until(stream, DEFAULT_LOG)

        def emit() -> None:
            for i in range(50):
                device.emit_log(f"line {i}: {'x' * 150}\n")

        emitter = threading.Thread(target=emit)
        emitter.start()
        for _ in range(20):
            nx.ping()  # would raise if a LOG frame were taken for the PONG
        emitter.join()
        stream.close()
