"""Documented S0 reference endpoint for independent-client testing.

This is an intentionally small oracle built from the accepted command tables.
It does not import the product endpoint or pyXCP's framing implementation.
"""

from __future__ import annotations

import errno
import os
import pty
import select
import threading
import time
from dataclasses import dataclass
from pathlib import Path

from .capture import Capture
from .framing import FrameError, StreamFrameDecoder, decode_frame, encode_frame
from .profile import (
    ACTIVE_REVISION,
    APPLIED_DUTY_PERMILLE,
    APPLIED_PERIOD_MS,
    BUILD_ID,
    CAPABILITIES,
    CLOCK_QUANTUM_US,
    COMMANDED_ON,
    DROPPED_SAMPLE_COUNT,
    DUTY_PERMILLE,
    EDGE_LATENESS_US,
    MAX_EDGE_LATENESS_US,
    PERIOD_MS,
    PHASE_US,
    PROFILE_ID,
    REJECTED_WRITE_COUNT,
    SAMPLE_SEQUENCE,
    SCHEMA_ID,
    REFERENCE_PTY_SXI,
    UPTIME_MS,
)


ERR_CMD_SYNCH = 0x00
ERR_CMD_BUSY = 0x10
ERR_CMD_UNKNOWN = 0x20
ERR_CMD_SYNTAX = 0x21
ERR_OUT_OF_RANGE = 0x22
ERR_WRITE_PROTECTED = 0x23
ERR_ACCESS_DENIED = 0x24


@dataclass
class EndpointState:
    period_ms: int = 1000
    duty_permille: int = 500
    active_revision: int = 0
    phase_us: int = 0
    rejected_write_count: int = 0
    sample_sequence: int = 0
    connected: bool = False
    mta: int = 0


class ReferenceEndpoint:
    def __init__(
        self,
        capture: Capture,
        *,
        period_address: int = PERIOD_MS,
        duty_address: int = DUTY_PERMILLE,
        build_id: bytes = bytes.fromhex("cd" * 32),
        schema_id: bytes = bytes.fromhex("ab" * 32),
        connect_response: bytes = bytes.fromhex("ff01000808000101"),
        status_response: bytes = bytes.fromhex("ff0000000000"),
    ):
        if len(build_id) != 32 or len(schema_id) != 32:
            raise ValueError("reference identities must each be exactly 32 bytes")
        self.capture = capture
        self.master_fd, slave_fd = pty.openpty()
        self.slave_path = Path(os.ttyname(slave_fd))
        os.close(slave_fd)
        os.set_blocking(self.master_fd, False)
        self.state = EndpointState()
        self._started_ns = time.monotonic_ns()
        self._response_counter = 0
        self._stop = threading.Event()
        self._thread = threading.Thread(target=self._run, name="s0-reference-endpoint", daemon=True)
        self._decoder = StreamFrameDecoder()
        self._last_byte_at = 0.0
        self._discard_until_idle = False
        self._rx_sequence = 0
        self.suppress_next_download_response = False
        self.period_address = period_address
        self.duty_address = duty_address
        self.build_id = build_id
        self.schema_id = schema_id
        self.connect_response = connect_response
        self.status_response = status_response

    def start(self) -> "ReferenceEndpoint":
        self._thread.start()
        return self

    def close(self) -> None:
        self._stop.set()
        self._thread.join(timeout=1.0)
        os.close(self.master_fd)

    def _run(self) -> None:
        while not self._stop.is_set():
            now = time.monotonic()
            if self._last_byte_at and now - self._last_byte_at >= REFERENCE_PTY_SXI.target_inter_byte_timeout_seconds:
                if self._decoder.buffer:
                    discarded = self._decoder.reset()
                    self.capture.raw("host_to_endpoint", discarded, status="discarded", detail="partial_frame_idle_timeout")
                self._discard_until_idle = False
                self._last_byte_at = 0.0
            try:
                ready, _, _ = select.select([self.master_fd], [], [], 0.005)
                if not ready:
                    continue
                data = os.read(self.master_fd, 256)
            except OSError as exc:
                if exc.errno in (errno.EIO, errno.EAGAIN):  # No slave yet, or a harmless readiness race.
                    time.sleep(0.005)
                    continue
                raise
            if not data:
                continue
            self._rx_sequence += 1
            self._last_byte_at = time.monotonic()
            if self._discard_until_idle:
                self.capture.raw("host_to_endpoint", data, status="discarded", detail="discard_until_idle")
                continue
            try:
                frames = self._decoder.feed(data)
            except FrameError as exc:
                discarded = self._decoder.reset()
                self.capture.raw("host_to_endpoint", discarded, status="rejected", detail=str(exc))
                self._discard_until_idle = True
                continue
            for frame in frames:
                self.capture.raw("host_to_endpoint", frame)
                _, pdu = decode_frame(frame)
                response = self._handle(pdu)
                if response is None:
                    continue
                response_frame = encode_frame(response, self._response_counter)
                self._response_counter = (self._response_counter + 1) & 0xFF
                os.write(self.master_fd, response_frame)
                self.capture.raw("endpoint_to_host", response_frame)

    def _positive(self, data: bytes = b"") -> bytes:
        return b"\xff" + data

    def _error(self, code: int) -> bytes:
        return bytes((0xFE, code))

    def _regions(self) -> dict[int, tuple[bytes, bool]]:
        uptime_ms = (time.monotonic_ns() - self._started_ns) // 1_000_000
        commanded = int(self.state.duty_permille != 0)
        return {
            BUILD_ID: (self.build_id, False),
            SCHEMA_ID: (self.schema_id, False),
            PROFILE_ID: ((1).to_bytes(4, "little"), False),
            CAPABILITIES: ((3).to_bytes(4, "little"), False),
            CLOCK_QUANTUM_US: ((100).to_bytes(4, "little"), False),
            self.period_address: (self.state.period_ms.to_bytes(2, "little"), True),
            self.duty_address: (self.state.duty_permille.to_bytes(2, "little"), True),
            UPTIME_MS: ((uptime_ms & 0xFFFF_FFFF).to_bytes(4, "little"), False),
            SAMPLE_SEQUENCE: (self.state.sample_sequence.to_bytes(4, "little"), False),
            ACTIVE_REVISION: (self.state.active_revision.to_bytes(4, "little"), False),
            PHASE_US: (self.state.phase_us.to_bytes(4, "little"), False),
            APPLIED_PERIOD_MS: (self.state.period_ms.to_bytes(2, "little"), False),
            APPLIED_DUTY_PERMILLE: (self.state.duty_permille.to_bytes(2, "little"), False),
            COMMANDED_ON: (commanded.to_bytes(1, "little"), False),
            REJECTED_WRITE_COUNT: (self.state.rejected_write_count.to_bytes(4, "little"), False),
            DROPPED_SAMPLE_COUNT: ((0).to_bytes(4, "little"), False),
            EDGE_LATENESS_US: ((0).to_bytes(4, "little"), False),
            MAX_EDGE_LATENESS_US: ((0).to_bytes(4, "little"), False),
        }

    def _find_read(self, address: int, count: int) -> bytes | None:
        end = address + count
        if end > 0x1_0000_0000:
            return None
        for start, (value, _) in self._regions().items():
            if start <= address and end <= start + len(value):
                offset = address - start
                return value[offset : offset + count]
        return None

    def _handle(self, pdu: bytes) -> bytes | None:
        command = pdu[0]
        self.capture.record("decoded-events.jsonl", direction="request", command=f"0x{command:02x}", pdu_hex=pdu.hex())
        if not self.state.connected and command != 0xFF:
            self.capture.record("decoded-events.jsonl", direction="response", behavior="ignored_while_disconnected")
            return None
        if command == 0xFF:  # CONNECT
            if pdu != b"\xff\x00":
                response = self._error(ERR_CMD_SYNTAX)
            else:
                self.state.connected = True
                response = self.connect_response
        elif command == 0xFE:  # DISCONNECT
            if len(pdu) != 1:
                response = self._error(ERR_CMD_SYNTAX)
            else:
                self.state.connected = False
                response = self._positive()
        elif command == 0xFD:  # GET_STATUS
            response = self.status_response if len(pdu) == 1 else self._error(ERR_CMD_SYNTAX)
        elif command == 0xFC:  # SYNCH
            response = self._error(ERR_CMD_SYNCH) if len(pdu) == 1 else self._error(ERR_CMD_SYNTAX)
        elif command == 0xF6:  # SET_MTA
            if len(pdu) != 8 or pdu[1:3] != b"\x00\x00":
                response = self._error(ERR_CMD_SYNTAX)
            elif pdu[3] != 0:
                response = self._error(ERR_OUT_OF_RANGE)
            else:
                self.state.mta = int.from_bytes(pdu[4:8], "little")
                response = self._positive()
        elif command == 0xF5:  # UPLOAD
            if len(pdu) != 2:
                response = self._error(ERR_CMD_SYNTAX)
            elif not 1 <= pdu[1] <= 7:
                response = self._error(ERR_OUT_OF_RANGE)
            elif self.state.mta + pdu[1] > 0x1_0000_0000:
                response = self._error(ERR_OUT_OF_RANGE)
            else:
                value = self._find_read(self.state.mta, pdu[1])
                if value is None:
                    response = self._error(ERR_ACCESS_DENIED)
                else:
                    self.state.mta += pdu[1]
                    response = self._positive(value)
        elif command == 0xF0:  # DOWNLOAD
            response = self._download(pdu)
            if response == self._positive() and self.suppress_next_download_response:
                self.suppress_next_download_response = False
                self.capture.record(
                    "decoded-events.jsonl", direction="response", behavior="suppressed_after_apply_for_reconciliation_test"
                )
                return None
        else:
            response = self._error(ERR_CMD_UNKNOWN)
        self.capture.record("decoded-events.jsonl", direction="response", pdu_hex=response.hex())
        return response

    def _download(self, pdu: bytes) -> bytes:
        if len(pdu) < 2:
            self.state.rejected_write_count += 1
            return self._error(ERR_CMD_SYNTAX)
        count = pdu[1]
        if len(pdu) != count + 2:
            self.state.rejected_write_count += 1
            return self._error(ERR_CMD_SYNTAX)
        if count != 2:
            self.state.rejected_write_count += 1
            return self._error(ERR_OUT_OF_RANGE)
        if self.state.mta + 2 > 0x1_0000_0000:
            self.state.rejected_write_count += 1
            return self._error(ERR_OUT_OF_RANGE)
        regions = self._regions()
        if self.state.mta in regions and not regions[self.state.mta][1]:
            self.state.rejected_write_count += 1
            return self._error(ERR_WRITE_PROTECTED)
        if self.state.mta not in (self.period_address, self.duty_address):
            self.state.rejected_write_count += 1
            return self._error(ERR_ACCESS_DENIED)
        value = int.from_bytes(pdu[2:4], "little")
        if self.state.mta == self.period_address:
            valid = 100 <= value <= 10_000
            old = self.state.period_ms
        else:
            valid = 0 <= value <= 1_000
            old = self.state.duty_permille
        if not valid:
            self.state.rejected_write_count += 1
            return self._error(ERR_OUT_OF_RANGE)
        if value != old:
            if self.state.mta == self.period_address:
                self.state.period_ms = value
            else:
                self.state.duty_permille = value
            self.state.active_revision = (self.state.active_revision + 1) & 0xFFFF_FFFF
            self.state.phase_us = ((time.monotonic_ns() - self._started_ns) // 1000) & 0xFFFF_FFFF
        self.state.mta += 2
        return self._positive()

    def observed_mta(self) -> int:
        """Reference-only state observation for otherwise unreadable MTA cases."""
        return self.state.mta

    def received_sequence(self) -> int:
        return self._rx_sequence

    def wait_for_rx_after(self, previous: int, timeout: float = 1.0) -> None:
        deadline = time.monotonic() + timeout
        while self._rx_sequence <= previous:
            if time.monotonic() >= deadline:
                raise TimeoutError("reference endpoint did not observe injected serial bytes")
            time.sleep(0.005)

    def wait_for_idle_recovery(self, timeout: float = 1.0) -> None:
        deadline = time.monotonic() + timeout
        while self._last_byte_at or self._decoder.buffer or self._discard_until_idle:
            if time.monotonic() >= deadline:
                raise TimeoutError("reference endpoint did not reach an observed idle parser boundary")
            time.sleep(0.005)
