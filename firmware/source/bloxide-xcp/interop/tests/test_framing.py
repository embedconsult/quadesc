import json
from pathlib import Path

import pytest

from xcp_s0_interop.framing import FrameError, StreamFrameDecoder, decode_frame, encode_frame


FIXTURES = Path(__file__).parents[1] / "fixtures"


def test_independent_golden_vectors():
    vectors = json.loads((FIXTURES / "golden-vectors.json").read_text())["vectors"]
    for vector in vectors:
        frame = encode_frame(bytes.fromhex(vector["pdu_hex"]), vector["counter"])
        assert frame.hex() == vector["frame_hex"]
        assert decode_frame(frame) == (vector["counter"], bytes.fromhex(vector["pdu_hex"]))


def test_recorded_pinned_client_capture_is_well_formed():
    capture = json.loads((FIXTURES / "captured-pyxcp-golden.json").read_text())
    assert capture["capture"]["client_commit"] == "016cf3e44364e9cd93966d144a39d342578a0391"
    for item in capture["exchange"]:
        decode_frame(bytes.fromhex(item["frame_hex"]))


def test_stream_decoder_and_errors():
    first = encode_frame(b"\xff\x00", 0)
    second = encode_frame(b"\xfd", 1)
    decoder = StreamFrameDecoder()
    assert decoder.feed(first[:2]) == []
    assert decoder.feed(first[2:] + second) == [first, second]
    assert [decode_frame(item)[0] for item in (first, second)] == [0, 1]
    damaged = bytearray(first)
    damaged[-1] ^= 1
    with pytest.raises(FrameError, match="SUM8"):
        decoder.feed(damaged)
    decoder.reset()
    with pytest.raises(FrameError, match="LEN"):
        decoder.feed(b"\x00")


@pytest.mark.parametrize("length", [0, 9])
def test_pdu_capacity(length):
    with pytest.raises(FrameError):
        encode_frame(bytes(length), 0)
