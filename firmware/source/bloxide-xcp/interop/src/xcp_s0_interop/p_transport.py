"""Owned synchronous restricted P transport for the unmodified pyXCP Master.

The upstream factory discovers this BaseTransport subclass. Its request builder,
error handling, SxI checksum receiver and Master parsers remain upstream code.
No asynchronous response queue or acquisition policy can qualify a transaction.
"""

from __future__ import annotations

import threading
import time
from contextlib import contextmanager
from dataclasses import dataclass
from typing import Iterator

from pyxcp import types
from pyxcp.transport.base import BaseTransport, ChecksumType, EmptyFrameError, XcpFramingConfig, XcpTransportLayerType
from pyxcp.transport.sxi import get_receiver_class

from .framing import decode_frame


class ResponseOwnershipError(AssertionError):
    """The session cannot safely associate or consume a response; reopen it."""


@dataclass(frozen=True)
class ConsumedResponse:
    request_counter: int
    request_pdu: bytes
    response_counter: int
    response_pdu: bytes


@dataclass
class _Exchange:
    request: bytes
    response: ConsumedResponse | None = None


class PSerialTransport(BaseTransport):
    """One caller owns write, receive, validation and the return to Master.

    P is sequential and has no unsolicited events, DAQ or block transfers.
    Extra input and uncertain I/O permanently fence this instance. A fresh
    serial open and fresh transport are required after failure or DISCONNECT.
    """

    def __init__(self, config, policy=None, transport_layer_interface=None):
        self._gate = threading.RLock()
        self._cancel = threading.Event()
        self._failure: str | None = None
        self._request_owner: int | None = None
        self._pending: tuple[int, bytes] | None = None
        self._scope: _Exchange | None = None
        self._opened = False
        self._disconnected = False
        self.config = config.sxi
        if (self.config.header_format, self.config.tail_format, config.alignment, self.config.framing) != (
            "HEADER_LEN_CTR_BYTE", "CHECKSUM_BYTE", 1, False
        ):
            raise ValueError("owned P transport requires LEN8/CTR8/SUM8, alignment1, no escaping")
        if transport_layer_interface is None:
            raise ValueError("owned P transport requires a capturing serial interface")
        super().__init__(
            config,
            XcpFramingConfig(
                transport_layer_type=XcpTransportLayerType.SXI,
                header_len=1, header_ctr=1, header_fill=0,
                tail_fill=False, tail_cs=ChecksumType.BYTE_CHECKSUM,
            ),
            policy,
            transport_layer_interface,
        )
        self.port = transport_layer_interface

    def _check_live(self) -> None:
        if self._cancel.is_set() or self._failure:
            raise ResponseOwnershipError(self._failure or "transport closed; open a new session")

    def _fence(self, reason: str) -> None:
        self._failure = self._failure or reason
        raise ResponseOwnershipError(self._failure)

    def _require_empty_input(self) -> None:
        self._check_live()
        # resQueue is inherited but never populated/consumed by this transport.
        # Refuse it too if an unsupported external producer has inserted data.
        if self.port.in_waiting or self.resQueue:
            self._fence("known input queued before write/consumption; open a new session")

    def _require_request(self) -> None:
        self._check_live()
        if self._request_owner != threading.get_ident():
            self._fence("send/get outside the owned request boundary")

    def connect(self) -> None:
        with self._gate:
            self._check_live()
            if self._disconnected:
                self._fence("DISCONNECT completed; open a new session")
            self._opened = True

    def listen(self) -> None:
        self._fence("P uses synchronous receive; listeners are unsupported")

    def close_connection(self) -> None:
        self.close()

    def close(self) -> None:
        # Signal before taking the lock: a blocked reader must observe close.
        self._cancel.set()
        with self._gate:
            if hasattr(self, "port"):
                self.port.close()

    @contextmanager
    def exchange(self, request: bytes) -> Iterator[_Exchange]:
        """Keep one exact assertion, including Master parsing, in one scope."""
        with self._gate:
            self._check_live()
            if self._scope is not None:
                self._fence("nested exact-response assertion")
            scope = _Exchange(request)
            self._scope = scope
            try:
                yield scope
                self._check_live()
                if scope.response is None:
                    self._fence("Master did not consume the scoped response")
            except BaseException as exc:
                self._failure = self._failure or f"exact-response assertion failed: {exc}"
                raise
            finally:
                self._scope = None

    def _request_internal(self, cmd, ignore_timeout=False, *data):
        with self._gate:
            self._check_live()
            if not self._opened or self._disconnected:
                self._fence("transport not open or DISCONNECT completed; open a new session")
            if self._request_owner is not None:
                self._fence("nested request during an active transaction")
            self._request_owner = threading.get_ident()
            try:
                result = super()._request_internal(cmd, ignore_timeout, *data)
                self._check_live()  # also prevents optional-response timeout from returning success
                if cmd == types.Command.DISCONNECT:
                    self._disconnected = True
                return result
            except types.XcpResponseError:
                # A well-framed negative response belongs to this request.
                raise
            except BaseException as exc:
                self._failure = self._failure or f"{type(exc).__name__}: {exc}"
                raise
            finally:
                self._request_owner = None
                self._pending = None

    def send(self, frame: bytes) -> None:
        self._require_request()
        self._require_empty_input()
        counter, request = decode_frame(frame)
        if self._scope is not None and (request != self._scope.request or self._scope.response is not None):
            self._fence("exact assertion must consume one response to its own request")
        self._pending = counter, request
        # The supplied interface checks after its capture hook and immediately
        # before serial.write, closing the old CMD/capture scheduling window.
        self.pre_send_timestamp = self.timestamp.value
        count = self.port.write_checked(frame, self._require_empty_input)
        self.post_send_timestamp = self.timestamp.value
        if count != len(frame):
            self._fence("partial serial write; command outcome uncertain")

    def get(self) -> bytes:
        self._require_request()
        if self._pending is None:
            self._fence("receive without a transmitted request")
        deadline = time.monotonic() + self.timeout / 1_000_000_000
        raw = bytearray()
        while True:
            self._check_live()
            if time.monotonic() >= deadline:
                self._failure = "response timeout; close and reopen required"
                raise EmptyFrameError
            # At most a complete P frame plus one extra byte. Reading a whole
            # available batch lets us reject a coalesced duplicate/partial next
            # frame before publishing the first response.
            raw.extend(self.port.read(max(1, min(self.port.in_waiting, 12))))
            self._check_live()
            if time.monotonic() >= deadline:
                self._failure = "response timeout; close and reopen required"
                raise EmptyFrameError
            if not raw:
                continue
            if not 1 <= raw[0] <= 8:
                self._fence("invalid P response length")
            total = raw[0] + 3
            if len(raw) > total:
                self._fence("extra response bytes; ambiguous transaction")
            if len(raw) < total:
                continue

            # One local callback result, consumed synchronously here. This is
            # the pinned upstream SxI receiver, not a replacement checksum rule.
            received: tuple[int, bytes] | None = None

            def dispatch(data, length, counter):
                nonlocal received
                if received is not None:
                    self._fence("duplicate response in one transaction")
                received = int(counter), bytes(data[:length])

            receiver = get_receiver_class("HEADER_LEN_CTR_BYTE", "CHECKSUM_BYTE")(dispatch)
            receiver.feed_bytes(bytes(raw))
            if received is None:
                self._fence("upstream SxI receiver rejected response checksum")
            counter, pdu = received
            if not pdu or pdu[0] not in (0xFF, 0xFE):
                self._fence("unsolicited non-response traffic in P")
            if pdu[0] == 0xFE and len(pdu) != 2:
                self._fence("malformed P error response")
            expected = {
                b"\xff\x00": ("CONNECT", bytes.fromhex("ff01000808000101")),

            }.get(self._pending[1])
            if expected and pdu != expected[1]:
                self._fence(f"{expected[0]} raw response mismatch: expected {expected[1].hex()}, got {pdu.hex()}")
            self._require_empty_input()
            response = ConsumedResponse(self._pending[0], self._pending[1], counter, pdu)
            self.counter_received = counter
            self.recv_timestamp = self.timestamp.value
            if self._scope is not None:
                self._scope.response = response
            # This very byte string is returned to inherited _request_internal,
            # which strips PID and returns it to the unchanged Master parser.
            return response.response_pdu

    def block_request(self, *args, **kwargs):
        with self._gate:
            self._fence("block requests are unsupported in P")

    def block_receive(self, *args, **kwargs):
        with self._gate:
            self._fence("block receive is unsupported in P")
