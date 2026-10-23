"""Button, D-pad and keyboard definitions."""

from __future__ import annotations

from enum import IntEnum, IntFlag


class Button(IntFlag):
    Y = 0x0001
    B = 0x0002
    A = 0x0004
    X = 0x0008
    L = 0x0010
    R = 0x0020
    ZL = 0x0040
    ZR = 0x0080
    MINUS = 0x0100
    PLUS = 0x0200
    LSTICK = 0x0400
    RSTICK = 0x0800
    HOME = 0x1000
    CAPTURE = 0x2000


class Hat(IntEnum):
    UP = 0
    UP_RIGHT = 1
    RIGHT = 2
    DOWN_RIGHT = 3
    DOWN = 4
    DOWN_LEFT = 5
    LEFT = 6
    UP_LEFT = 7
    CENTER = 8


STICK_CENTER = 128


class Modifier(IntFlag):
    LCTRL = 0x01
    LSHIFT = 0x02
    LALT = 0x04
    LGUI = 0x08
    RCTRL = 0x10
    RSHIFT = 0x20
    RALT = 0x40
    RGUI = 0x80


class Key(IntEnum):
    """USB HID keyboard usage IDs (usage page 0x07)."""

    A = 0x04
    B = 0x05
    C = 0x06
    D = 0x07
    E = 0x08
    F = 0x09
    G = 0x0A
    H = 0x0B
    I = 0x0C  # noqa: E741
    J = 0x0D
    K = 0x0E
    L = 0x0F
    M = 0x10
    N = 0x11
    O = 0x12  # noqa: E741
    P = 0x13
    Q = 0x14
    R = 0x15
    S = 0x16
    T = 0x17
    U = 0x18
    V = 0x19
    W = 0x1A
    X = 0x1B
    Y = 0x1C
    Z = 0x1D
    N1 = 0x1E
    N2 = 0x1F
    N3 = 0x20
    N4 = 0x21
    N5 = 0x22
    N6 = 0x23
    N7 = 0x24
    N8 = 0x25
    N9 = 0x26
    N0 = 0x27
    ENTER = 0x28
    ESCAPE = 0x29
    BACKSPACE = 0x2A
    TAB = 0x2B
    SPACE = 0x2C
    MINUS = 0x2D
    EQUAL = 0x2E
    LEFT_BRACKET = 0x2F
    RIGHT_BRACKET = 0x30
    BACKSLASH = 0x31
    SEMICOLON = 0x33
    APOSTROPHE = 0x34
    GRAVE = 0x35
    COMMA = 0x36
    PERIOD = 0x37
    SLASH = 0x38
    CAPS_LOCK = 0x39
    F1 = 0x3A
    F2 = 0x3B
    F3 = 0x3C
    F4 = 0x3D
    F5 = 0x3E
    F6 = 0x3F
    F7 = 0x40
    F8 = 0x41
    F9 = 0x42
    F10 = 0x43
    F11 = 0x44
    F12 = 0x45
    PRINT_SCREEN = 0x46
    SCROLL_LOCK = 0x47
    PAUSE = 0x48
    INSERT = 0x49
    HOME = 0x4A
    PAGE_UP = 0x4B
    DELETE = 0x4C
    END = 0x4D
    PAGE_DOWN = 0x4E
    RIGHT = 0x4F
    LEFT = 0x50
    DOWN = 0x51
    UP = 0x52


_UNSHIFTED_SYMBOLS = {
    " ": Key.SPACE,
    "\n": Key.ENTER,
    "\t": Key.TAB,
    "-": Key.MINUS,
    "=": Key.EQUAL,
    "[": Key.LEFT_BRACKET,
    "]": Key.RIGHT_BRACKET,
    "\\": Key.BACKSLASH,
    ";": Key.SEMICOLON,
    "'": Key.APOSTROPHE,
    "`": Key.GRAVE,
    ",": Key.COMMA,
    ".": Key.PERIOD,
    "/": Key.SLASH,
}
_SHIFTED_SYMBOLS = dict(zip('!@#$%^&*()_+{}|:"~<>?', "1234567890-=[]\\;'`,./"))
_DIGITS = [Key.N0, Key.N1, Key.N2, Key.N3, Key.N4, Key.N5, Key.N6, Key.N7, Key.N8, Key.N9]


def char_to_key(ch: str) -> tuple[Modifier, Key]:
    """Map a character to the (modifiers, key) needed to type it on a US layout."""
    if "a" <= ch <= "z":
        return Modifier(0), Key[ch.upper()]
    if "A" <= ch <= "Z":
        return Modifier.LSHIFT, Key[ch]
    if "0" <= ch <= "9":
        return Modifier(0), _DIGITS[int(ch)]
    if ch in _UNSHIFTED_SYMBOLS:
        return Modifier(0), _UNSHIFTED_SYMBOLS[ch]
    if ch in _SHIFTED_SYMBOLS:
        _, key = char_to_key(_SHIFTED_SYMBOLS[ch])
        return Modifier.LSHIFT, key
    raise ValueError(f"cannot type character {ch!r} on a US keyboard layout")


def parse_button(value: Button | str) -> Button:
    if isinstance(value, Button):
        return value
    try:
        return Button[value.upper()]
    except KeyError:
        raise ValueError(f"unknown button {value!r}; choose from {[b.name for b in Button]}") from None


def parse_hat(value: Hat | str) -> Hat:
    if isinstance(value, Hat):
        return value
    try:
        return Hat[value.upper().replace("-", "_")]
    except KeyError:
        raise ValueError(f"unknown d-pad direction {value!r}") from None


def parse_key(value: Key | str) -> Key:
    if isinstance(value, Key):
        return value
    name = value.upper()
    if len(name) == 1 and name.isdigit():
        name = "N" + name
    try:
        return Key[name]
    except KeyError:
        raise ValueError(f"unknown key {value!r}") from None
