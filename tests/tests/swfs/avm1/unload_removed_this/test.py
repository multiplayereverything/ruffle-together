"""Writes test.swf by hand (no Flash here): a duplicated clip runs removeMovieClip(this); unloadMovie(this);"""
import struct


def push(*values):
    data = b""
    for value in values:
        data += b"\x07" + struct.pack("<i", value) if isinstance(value, int) else b"\x00" + value.encode() + b"\0"
    return b"\x96" + struct.pack("<H", len(data)) + data


def tag(code, data=b""):
    return struct.pack("<H", code << 6 | 63) + struct.pack("<I", len(data)) + data


GET_VARIABLE, TRACE, REMOVE_SPRITE, CLONE_SPRITE, EQUALS2, NOT, STOP = b"\x1c", b"\x26", b"\x25", b"\x24", b"\x49", b"\x12", b"\x07"
UNLOAD_THIS = push("", "this") + GET_VARIABLE + b"\x9a\x01\x00\x40"
body = (
    push("this") + GET_VARIABLE + REMOVE_SPRITE + UNLOAD_THIS + push("copy ran on") + TRACE
)
clip_script = (
    push("_name") + GET_VARIABLE + push("copy") + EQUALS2 + NOT + b"\x9d\x02\x00" + struct.pack("<h", len(body)) + body + b"\0"
)
clip = struct.pack("<HH", 1, 1) + tag(12, clip_script) + tag(1) + tag(0)
frame1 = push("template", "copy", 16384 + 10) + CLONE_SPRITE + b"\0"
frame2 = push("root still here, frame") + TRACE + push("_currentframe") + GET_VARIABLE + TRACE
frame2 += push("_totalframes") + GET_VARIABLE + TRACE + STOP + b"\0"
tags = (
    tag(69, struct.pack("<I", 0))
    + tag(39, clip)
    + tag(26, b"\x22" + struct.pack("<HH", 1, 1) + b"template\0")
    + tag(12, frame1)
    + tag(1)
    + tag(12, frame2)
    + tag(1)
    + tag(1)
    + tag(0)
)
body = b"\0" + struct.pack("<HH", 24 << 8, 3) + tags
open("test.swf", "wb").write(b"FWS\x08" + struct.pack("<I", 8 + len(body)) + body)
