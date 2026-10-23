import time

import pytest

from nxcontroller import Button, Controller, Hat, Key, Modifier, NXError
from nxcontroller import protocol as p
from nxcontroller.inputs import char_to_key
from nxcontroller.mock import MockDevice


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
