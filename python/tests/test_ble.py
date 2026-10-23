"""Transport-independent parts of nxcontroller.ble (no bleak or radio needed)."""

from nxcontroller import protocol as p
from nxcontroller.ble import FrameBuffer, chunks


def test_frames_split_across_notifications():
    buf = FrameBuffer()
    data = p.frame(p.MSG_PONG, bytes([1, 0, 0, 0, 1])) + p.frame(p.MSG_WIFI_ACK)
    assert buf.feed(data[:1]) == []
    assert buf.feed(data[1:4]) == []
    assert buf.feed(data[4:]) == [(p.MSG_PONG, bytes([1, 0, 0, 0, 1])), (p.MSG_WIFI_ACK, b"")]


def test_several_frames_in_one_notification():
    buf = FrameBuffer()
    data = p.encode_ping(1) + p.encode_release_all() + p.encode_ping(2)[:3]
    frames = buf.feed(data)
    assert [t for t, _ in frames] == [p.MSG_PING, p.MSG_RELEASE_ALL]
    assert buf.feed(p.encode_ping(2)[3:]) == [(p.MSG_PING, p.encode_ping(2)[2:])]


def test_chunks_cover_long_frames():
    frame = p.frame(p.MSG_SET_WIFI, bytes(255))
    parts = chunks(frame, 20)
    assert b"".join(parts) == frame
    assert all(len(c) <= 20 for c in parts)
