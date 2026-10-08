"""S0 framing helpers, implemented independently from pyXCP."""

from __future__ import annotations

from dataclasses import dataclass


MAX_PDU = 8


class FrameError(ValueError):
    pass


def encode_frame(pdu: bytes, counter: int) -> bytes:
    if not 1 <= len(pdu) <= MAX_PDU:
        raise FrameError("S0 PDU length must be in 1..8")
    if not 0 <= counter <= 0xFF:
        raise FrameError("counter must fit u8")
    body = bytes((len(pdu), counter)) + pdu
    return body + bytes((sum(body) & 0xFF,))


def decode_frame(frame: bytes) -> tuple[int, bytes]:
    if len(frame) < 4:
        raise FrameError("frame is shorter than LEN/CTR/PDU/SUM")
    length = frame[0]
    if not 1 <= length <= MAX_PDU:
        raise FrameError("LEN is outside 1..8")
    if len(frame) != length + 3:
        raise FrameError("frame length does not match LEN")
    if (sum(frame[:-1]) & 0xFF) != frame[-1]:
        raise FrameError("SUM8 mismatch")
    return frame[1], frame[2:-1]


@dataclass
class StreamFrameDecoder:
    """Extract complete S0 frames; the caller owns idle-boundary recovery."""

    buffer: bytearray

    def __init__(self) -> None:
        self.buffer = bytearray()

    def feed(self, data: bytes) -> list[bytes]:
        self.buffer.extend(data)
        frames: list[bytes] = []
        while self.buffer:
            length = self.buffer[0]
            if not 1 <= length <= MAX_PDU:
                raise FrameError("LEN is outside 1..8")
            total = length + 3
            if len(self.buffer) < total:
                break
            frame = bytes(self.buffer[:total])
            decode_frame(frame)
            del self.buffer[:total]
            frames.append(frame)
        return frames

    def reset(self) -> bytes:
        discarded = bytes(self.buffer)
        self.buffer.clear()
        return discarded
