"""Literal-byte F3 regression independent of product client/framing helpers."""

from __future__ import annotations

import serial

from xcp_s0_interop.capture import Capture
from xcp_s0_interop.simulator import ReferenceEndpoint


def _frame(pdu: bytes, counter: int) -> bytes:
    body = bytes((len(pdu), counter & 0xFF)) + pdu
    return body + bytes((sum(body) & 0xFF,))


def _read_frame(port: serial.Serial) -> tuple[int, bytes]:
    header = port.read(2)
    assert len(header) == 2
    tail = port.read(header[0] + 1)
    assert len(tail) == header[0] + 1
    raw = header + tail
    assert sum(raw[:-1]) & 0xFF == raw[-1]
    return raw[1], raw[2:-1]


class _RawClient:
    def __init__(self, port: serial.Serial):
        self.port = port
        self.counter = 0

    def request(self, pdu: bytes) -> tuple[int, bytes]:
        self.port.write(_frame(pdu, self.counter))
        self.port.flush()
        self.counter = (self.counter + 1) & 0xFF
        return _read_frame(self.port)

    def set_mta(self, address: int) -> None:
        _, response = self.request(bytes.fromhex("f6000000") + address.to_bytes(4, "little"))
        assert response == bytes.fromhex("ff")

    def upload(self, address: int, width: int) -> bytes:
        self.set_mta(address)
        _, response = self.request(bytes((0xF5, width)))
        assert response[0] == 0xFF
        return response[1:]


def test_frozen_download_error_table_and_state_invariants(tmp_path):
    endpoint = ReferenceEndpoint(Capture(tmp_path / "raw-normative-oracle")).start()
    port = serial.Serial(str(endpoint.slave_path), 38_400, timeout=2.0, write_timeout=0.5)
    client = _RawClient(port)
    try:
        _, response = client.request(bytes.fromhex("ff00"))
        assert response == bytes.fromhex("ff01000808000101")

        def state() -> tuple[bytes, bytes, bytes]:
            return (client.upload(0x1000, 2), client.upload(0x1002, 2), client.upload(0x2008, 4))

        cases = [
            ("semantic_count_0", bytes.fromhex("f000"), bytes.fromhex("fe22")),
            ("semantic_count_1", bytes.fromhex("f00100"), bytes.fromhex("fe22")),
            ("semantic_count_3", bytes.fromhex("f003000000"), bytes.fromhex("fe22")),
            ("semantic_count_4", bytes.fromhex("f00400000000"), bytes.fromhex("fe22")),
            ("syntax_missing_count", bytes.fromhex("f0"), bytes.fromhex("fe21")),
            ("syntax_payload_short", bytes.fromhex("f00200"), bytes.fromhex("fe21")),
            ("syntax_payload_long", bytes.fromhex("f0010000"), bytes.fromhex("fe21")),
        ]
        for name, request, expected in cases:
            before = state()
            client.set_mta(0x1000)
            _, observed = client.request(request)
            assert observed == expected, name
            # No intervening SET_MTA: this read proves the failed transfer kept
            # MTA at the readable period scalar as well as preserving its value.
            _, retained = client.request(bytes.fromhex("f502"))
            assert retained == bytes.fromhex("ffe803"), name
            assert state() == before, name
    finally:
        port.close()
        endpoint.close()
