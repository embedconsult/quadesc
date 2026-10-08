"""Real pyXCP/PTTY ordering regressions with an independently framed peer."""

import errno
import os
import pty
import select
import sys
import threading
import time
from contextlib import contextmanager
from pathlib import Path

import pytest
from pyxcp import types
from pyxcp.transport.base import BaseTransport

from xcp_s0_interop.a2lcheck import parse_a2l
from xcp_s0_interop.bundle import load_bundle_identity
from xcp_s0_interop.capture import Capture
from xcp_s0_interop.client import ClientSession, assert_connect, assert_get_status, run_external_s0
from xcp_s0_interop.profile import EXTERNAL_19200_SXI, REFERENCE_PTY_SXI
from xcp_s0_interop.transport import ResponseOwnershipError, S0SerialTransport


GOOD_CONNECT = bytes.fromhex("ff01000808000101")
GOOD_STATUS = bytes.fromhex("ff0000000000")
FIXTURES = Path(__file__).parents[1] / "fixtures"


def wire(pdu, counter=0x91):
    raw = bytes([len(pdu), counter]) + pdu
    return raw + bytes([sum(raw) & 255])


def until(predicate):
    deadline = time.monotonic() + 2
    while not predicate():
        if time.monotonic() >= deadline:
            raise AssertionError("PTY scheduling predicate timed out")
        time.sleep(0.001)


class ScheduledCapture(Capture):
    hook = None

    def record(self, name, **entry):
        super().record(name, **entry)
        if self.hook:
            self.hook(entry)


class Peer:
    """No product simulator, framing helper, addresses or metadata consumed."""

    def __init__(self, root):
        self.capture = Capture(root)
        self.fd, slave = pty.openpty()
        self.path = os.ttyname(slave)
        os.close(slave)
        self.requests = []
        self.reply = self.normal
        self.stop = threading.Event()
        self.error = None
        self.thread = threading.Thread(target=self.run, daemon=True)
        self.thread.start()

    def normal(self, pdu):
        return wire({b"\xff\x00": GOOD_CONNECT, b"\xfd": GOOD_STATUS, b"\xfe": b"\xff"}.get(pdu, b"\xfe\x24"))

    def emit(self, raw):
        os.write(self.fd, raw)
        self.capture.record("peer-wire.jsonl", direction="endpoint_to_host", raw=raw.hex())

    def run(self):
        buffer = bytearray()
        try:
            while not self.stop.is_set():
                if not select.select([self.fd], [], [], 0.01)[0]:
                    continue
                try:
                    data = os.read(self.fd, 4096)
                except OSError as exc:
                    if exc.errno == errno.EIO:
                        time.sleep(0.001)
                        continue
                    raise
                buffer.extend(data)
                while buffer and len(buffer) >= buffer[0] + 3:
                    count = buffer[0] + 3
                    raw = bytes(buffer[:count])
                    del buffer[:count]
                    assert sum(raw[:-1]) & 255 == raw[-1]
                    request = raw[2:-1]
                    self.capture.record("peer-wire.jsonl", direction="host_to_endpoint", raw=raw.hex())
                    self.requests.append(request)
                    response = self.reply(request)
                    if response:
                        self.emit(response)
        except BaseException as exc:
            self.error = exc

    def close(self):
        self.stop.set()
        self.thread.join(2)
        assert not self.thread.is_alive()
        os.close(self.fd)
        self.capture.finalize_manifest()
        if self.error:
            raise self.error


@contextmanager
def opened(tmp_path):
    peer = Peer(tmp_path / "peer")
    capture = ScheduledCapture(tmp_path / "client")
    session = ClientSession(peer.path, capture, REFERENCE_PTY_SXI)
    try:
        assert isinstance(session.master.transport, S0SerialTransport)
        assert not session.master.transport.listener.is_alive()
        yield peer, capture, session
    finally:
        session.close()
        peer.close()
        capture.finalize_manifest()


@pytest.mark.parametrize("command", ["connect", "status"])
@pytest.mark.parametrize("when", ["before_write", "already_queued"])
def test_known_prewrite_input_fences_before_command_or_identity(tmp_path, command, when):
    with opened(tmp_path) as (peer, capture, session):
        if command == "status":
            assert_connect(session.master)
        before = list(peer.requests)
        good = GOOD_CONNECT if command == "connect" else GOOD_STATUS
        bad = bytes.fromhex("ff03000808000101" if command == "connect" else "ff0002000000")
        peer.reply = lambda request: wire(bad if when == "before_write" else good)

        def inject(entry=None):
            if entry is not None and entry.get("direction") != "host_to_endpoint":
                return
            capture.hook = None
            assert peer.requests == before
            peer.emit(wire(good if when == "before_write" else bad, 0x80))
            until(lambda: session.serial.in_waiting)
            assert peer.requests == before

        if when == "before_write":
            capture.hook = inject
        else:
            inject()
        with pytest.raises(ResponseOwnershipError, match="known input queued"):
            (assert_connect if command == "connect" else assert_get_status)(session.master)
        # Real raw request covers refused CONNECT too: Master has no packers yet.
        with pytest.raises(ResponseOwnershipError):
            session.master.transport.request(types.Command.SET_MTA, 0, 0, 0, 0, 0, 0, 0)
        assert peer.requests == before
        assert not session.master.transport.resQueue


def test_external_workflow_refuses_before_identity_after_prewrite_status(tmp_path):
    peer = Peer(tmp_path / "peer")
    capture = ScheduledCapture(tmp_path / "client")
    peer.reply = lambda pdu: wire(bytes.fromhex("ff0002000000")) if pdu == b"\xfd" else peer.normal(pdu)

    def inject(entry):
        if entry.get("direction") != "host_to_endpoint" or bytes.fromhex(entry["raw_chunk_hex"])[2:-1] != b"\xfd":
            return
        capture.hook = None
        assert peer.requests == [b"\xff\x00"]
        peer.emit(wire(GOOD_STATUS, 0x90))
        # Deterministic kernel delivery barrier using the current serial fd.
        # No observer/queue mutation: only inspect the write_checked call frame.
        frame = sys._getframe()
        while frame.f_code.co_name != "write_checked":
            frame = frame.f_back
        port = frame.f_locals["self"]
        del frame
        until(lambda: port.in_waiting)
        assert peer.requests == [b"\xff\x00"]

    capture.hook = inject
    try:
        with pytest.raises(ResponseOwnershipError, match="known input queued"):
            run_external_s0(
                peer.path, capture,
                parse_a2l(FIXTURES / "led-s0.a2l", FIXTURES / "descriptors.json"),
                load_bundle_identity(FIXTURES / "bundle-metadata.json"), EXTERNAL_19200_SXI,
            )
        assert peer.requests == [b"\xff\x00"]
    finally:
        peer.close()
        capture.finalize_manifest()


def test_fragmented_response_receipt_equals_upstream_consumption(tmp_path):
    with opened(tmp_path) as (peer, capture, session):
        def fragmented(pdu):
            for octet in peer.normal(pdu):
                peer.emit(bytes([octet]))
                time.sleep(0.002)
            return None

        peer.reply = fragmented
        returns = []

        def trace(frame, event, value):
            if event == "return" and frame.f_code is BaseTransport._request_internal.__code__ and isinstance(value, bytes):
                returns.append(value)
            return trace

        sys.settrace(trace)
        try:
            connect = assert_connect(session.master)
            status = assert_get_status(session.master)
        finally:
            sys.settrace(None)
        assert returns == [GOOD_CONNECT[1:], GOOD_STATUS[1:]]
        assert connect["raw_pdu"] == GOOD_CONNECT.hex()
        assert status["raw_pdu"] == GOOD_STATUS.hex()
        assert connect["request_counter"] == 0 and status["request_counter"] == 1
        assert connect["response_counter"] == status["response_counter"] == 0x91
        assert connect["resource"]["calpag"] and status["resource_protection"]["calpag"] is False
        assert session.master.transport._scope is None
        assert not session.master.transport.resQueue


@pytest.mark.parametrize("variant", ["duplicate", "partial_extra", "checksum", "length", "event"])
def test_ambiguous_or_malformed_real_frames_fence(tmp_path, variant):
    with opened(tmp_path) as (peer, capture, session):
        raw = wire(GOOD_CONNECT)
        raw = {
            "duplicate": raw + wire(GOOD_CONNECT, 0x92),
            "partial_extra": raw + b"\x06",
            "checksum": raw[:-1] + bytes([raw[-1] ^ 1]),
            "length": wire(GOOD_CONNECT + b"\x00"),
            "event": wire(b"\xfd\x00"),
        }[variant]
        peer.reply = lambda request: raw
        with pytest.raises(ResponseOwnershipError):
            assert_connect(session.master)
        with pytest.raises(ResponseOwnershipError):
            session.master.transport.request(types.Command.SET_MTA, 0, 0, 0, 0, 0, 0, 0)
        assert peer.requests == [b"\xff\x00"]


def test_duplicate_observed_after_consumption_fences_next_command(tmp_path):
    with opened(tmp_path) as (peer, capture, session):
        assert_connect(session.master)
        assert_get_status(session.master)
        peer.emit(wire(GOOD_STATUS, 0x93))
        until(lambda: session.serial.in_waiting)
        with pytest.raises(ResponseOwnershipError):
            session.master.setMta(0)
        assert peer.requests == [b"\xff\x00", b"\xfd"]


@pytest.mark.parametrize("ending", ["timeout", "disconnect", "close"])
def test_terminal_session_cannot_revive_and_fresh_open_works(tmp_path, ending):
    with opened(tmp_path) as (peer, capture, session):
        assert_connect(session.master)
        if ending == "timeout":
            peer.reply = lambda pdu: None
            with pytest.raises(types.XcpTimeoutError):
                session.master.getStatus()
            peer.emit(wire(GOOD_STATUS, 0xB0))
            until(lambda: session.serial.in_waiting)
        elif ending == "disconnect":
            session.master.disconnect()
        else:
            peer.reply = lambda pdu: None
            outcome = []

            def request():
                try:
                    assert_get_status(session.master)
                except BaseException as exc:
                    outcome.append(exc)

            worker = threading.Thread(target=request)
            worker.start()
            until(lambda: b"\xfd" in peer.requests)
            session.close()
            worker.join(2)
            assert not worker.is_alive()
            assert len(outcome) == 1 and isinstance(outcome[0], ResponseOwnershipError)
        before = list(peer.requests)
        with pytest.raises(ResponseOwnershipError):
            session.master.getStatus()
        with pytest.raises(ResponseOwnershipError):
            session.master.connect()
        assert peer.requests == before
        session.close()
        peer.reply = peer.normal
        fresh = ClientSession(peer.path, capture, REFERENCE_PTY_SXI)
        try:
            assert_connect(fresh.master)
            assert_get_status(fresh.master)
        finally:
            fresh.close()


def test_concurrent_exact_scopes_own_their_receipts(tmp_path):
    with opened(tmp_path) as (peer, capture, session):
        assert_connect(session.master)
        start = threading.Barrier(3)
        outcomes = []

        def call():
            start.wait(2)
            try:
                outcomes.append(assert_get_status(session.master))
            except BaseException as exc:
                outcomes.append(exc)

        threads = [threading.Thread(target=call) for _ in range(2)]
        for thread in threads:
            thread.start()
        start.wait(2)
        for thread in threads:
            thread.join(2)
            assert not thread.is_alive()
        assert len(outcomes) == 2 and all(isinstance(item, dict) for item in outcomes)
        assert {item["request_counter"] for item in outcomes} == {1, 2}
        assert all(item["raw_pdu"] == GOOD_STATUS.hex() for item in outcomes)


def test_nested_capture_request_fences_outer_before_write(tmp_path):
    with opened(tmp_path) as (peer, capture, session):
        assert_connect(session.master)

        def nested(entry):
            if entry.get("direction") != "host_to_endpoint":
                return
            capture.hook = None
            with pytest.raises(ResponseOwnershipError, match="nested request"):
                session.master.getStatus()

        capture.hook = nested
        with pytest.raises(ResponseOwnershipError, match="nested request"):
            session.master.getStatus()
        assert peer.requests == [b"\xff\x00"]
