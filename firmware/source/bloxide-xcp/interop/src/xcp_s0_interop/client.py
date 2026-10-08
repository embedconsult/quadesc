"""Pinned pyXCP client driver and reusable, metadata-driven S0 operations."""

from __future__ import annotations

import logging
from dataclasses import asdict, dataclass
from pathlib import Path
from typing import Any, Callable

import serial
from pyxcp import Master, types
from pyxcp.config import create_application_from_config

from .a2lcheck import ParsedA2L, ScalarMetadata
from .bundle import BundleIdentity
from .capture import Capture
from .framing import StreamFrameDecoder
from .profile import SxiConfig
from .transport import ResponseOwnershipError, S0SerialTransport


class IdentityMismatchError(RuntimeError):
    """The connected firmware does not match the selected companion bundle."""


@dataclass(frozen=True)
class VerifiedIdentity:
    build_id: str
    schema_id: str


class CapturingSerial:
    """Capture raw serial bytes and check admission immediately before each write."""

    def __init__(self, port: str, capture: Capture, config: SxiConfig):
        self._serial = serial.Serial(
            port=port,
            baudrate=config.bitrate,
            bytesize=config.bytesize,
            parity=config.parity,
            stopbits=config.stopbits,
            timeout=0.01,
            write_timeout=config.timeout_seconds,
        )
        self.capture = capture
        self.portstr = self._serial.portstr

    @property
    def in_waiting(self) -> int:
        return self._serial.in_waiting

    @property
    def is_open(self) -> bool:
        return self._serial.is_open

    def read(self, size: int = 1) -> bytes:
        data = self._serial.read(size)
        if data:
            self.capture.record(
                "host-wire.jsonl", transport="uart", direction="endpoint_to_host", raw_chunk_hex=data.hex(), octets=len(data)
            )
        return data

    def write_checked(self, data: bytes, before_write: Callable[[], None]) -> int:
        self.capture.record(
            "host-wire.jsonl", transport="uart", direction="host_to_endpoint", raw_chunk_hex=bytes(data).hex(), octets=len(data), stage="write_attempt"
        )
        before_write()
        written = self._serial.write(data)
        self.capture.record("transport-events.jsonl", event="serial_write_complete", octets=written)
        return written

    def flush(self) -> None:
        self._serial.flush()

    def close(self) -> None:
        self._serial.close()


def build_application(port: str, config: SxiConfig):
    app = create_application_from_config(
        {
            "Transport": {
                "SxI": {
                    "port": port,
                    "bitrate": config.bitrate,
                    "bytesize": config.bytesize,
                    "parity": config.parity,
                    "stopbits": config.stopbits,
                    "mode": config.mode,
                    "header_format": config.header_format,
                    "tail_format": config.tail_format,
                    "framing": config.framing,
                }
            },
            "General": {"disable_error_handling": True, "diagnostics_on_failure": False},
        },
        log_level=logging.ERROR,
    )
    # The pinned helper applies nested transport settings but not these generic
    # Transport fields, so set them explicitly without modifying upstream.
    app.transport.layer = "SXI"
    app.transport.timeout = config.timeout_seconds
    app.transport.alignment = config.alignment
    return app


class ClientSession:
    def __init__(self, port: str, capture: Capture, config: SxiConfig):
        self.serial = CapturingSerial(port, capture, config)
        try:
            self.master = Master(
                "s0serialtransport", config=build_application(port, config), transport_layer_interface=self.serial
            )
            self.master.transport.connect()
        except BaseException:
            self.serial.close()
            raise

    def close(self) -> None:
        self.master.transport.close()


def error_code(exc: types.XcpResponseError) -> int:
    return int(exc.get_error_code())


def expect_error(call: Callable[[], Any], expected: int) -> None:
    try:
        call()
    except types.XcpResponseError as exc:
        actual = error_code(exc)
        if actual != expected:
            raise AssertionError(f"expected XCP error 0x{expected:02x}, got 0x{actual:02x}") from exc
    else:
        raise AssertionError(f"expected XCP error 0x{expected:02x}")


def read_bytes(master: Master, address: int, count: int, address_extension: int = 0) -> bytes:
    """Read a mapped region using legal S0 UPLOAD chunks (maximum seven data bytes)."""
    master.setMta(address, address_extension)
    output = bytearray()
    while len(output) < count:
        output.extend(master.upload(min(7, count - len(output))))
    return bytes(output)


def read_scalar(master: Master, scalar: ScalarMetadata) -> int:
    return scalar.decode(read_bytes(master, scalar.address, scalar.width, scalar.address_extension))


def write_scalar(master: Master, scalar: ScalarMetadata, value: int, identity: VerifiedIdentity) -> None:
    if not identity.build_id or not identity.schema_id:
        raise IdentityMismatchError("a verified build/schema identity is required before DOWNLOAD")
    if scalar.kind != "CHARACTERISTIC" or scalar.access != "calibration_read_write":
        raise ValueError(f"{scalar.symbol} is not a writable calibration scalar")
    payload = scalar.encode(value)
    master.setMta(scalar.address, scalar.address_extension)
    master.download(payload)


def verify_bundle_identity(master: Master, expected: BundleIdentity) -> VerifiedIdentity:
    # Read both fixed blobs completely before deciding. This produces one
    # coherent mismatch report while still failing closed before any DOWNLOAD.
    observed = {
        blob.name: read_bytes(master, blob.address, len(blob.value))
        for blob in expected.blobs()
    }
    mismatches = [
        f"{blob.name} mismatch: expected {blob.value.hex()}, observed {observed[blob.name].hex()}"
        for blob in expected.blobs()
        if observed[blob.name] != blob.value
    ]
    if mismatches:
        raise IdentityMismatchError("; ".join(mismatches) + "; writes disabled")
    return VerifiedIdentity(
        build_id=observed["build_id"].hex(),
        schema_id=observed["schema_id"].hex(),
    )


def _owned_transport(master: Master) -> S0SerialTransport:
    transport = master.transport
    if not isinstance(transport, S0SerialTransport):
        raise ResponseOwnershipError("exact S0 assertions require the owned synchronous transport")
    return transport


def assert_connect(master: Master) -> dict[str, Any]:
    with _owned_transport(master).exchange(bytes.fromhex("ff00")) as exchange:
        response = master.connect()
        raw = exchange.response
        if raw is None:
            raise ResponseOwnershipError("Master returned without a consumed response")
        observed = {
            "raw_pdu": raw.response_pdu.hex(),
            "request_counter": raw.request_counter,
            "response_counter": raw.response_counter,
            "max_cto": int(response.maxCto),
            "max_dto": int(response.maxDto),
            "protocol_version": int(response.protocolLayerVersion),
            "transport_version": int(response.transportLayerVersion),
            "resource": {
                "calpag": bool(response.resource.calpag),
                "daq": bool(response.resource.daq),
                "stim": bool(response.resource.stim),
                "pgm": bool(response.resource.pgm),
                "dbg": bool(response.resource.dbg),
            },
            "comm_mode_basic": {
                "byte_order": str(response.commModeBasic.byteOrder),
                "address_granularity": str(response.commModeBasic.addressGranularity),
                "slave_block_mode": bool(response.commModeBasic.slaveBlockMode),
                "optional": bool(response.commModeBasic.optional),
            },
        }
        expected = {
            "raw_pdu": "ff01000808000101",
            "request_counter": raw.request_counter,
            "response_counter": raw.response_counter,
            "max_cto": 8,
            "max_dto": 8,
            "protocol_version": 0x01,
            "transport_version": 0x01,
            "resource": {"calpag": True, "daq": False, "stim": False, "pgm": False, "dbg": False},
            "comm_mode_basic": {
                "byte_order": "INTEL",
                "address_granularity": "BYTE",
                "slave_block_mode": False,
                "optional": False,
            },
        }
        if observed != expected:
            raise AssertionError(f"CONNECT profile mismatch: expected {expected!r}, got {observed!r}")
        return observed


def assert_get_status(master: Master) -> dict[str, Any]:
    with _owned_transport(master).exchange(bytes.fromhex("fd")) as exchange:
        response = master.getStatus()
        raw = exchange.response
        if raw is None:
            raise ResponseOwnershipError("Master returned without a consumed response")
        observed = {
            "raw_pdu": raw.response_pdu.hex(),
            "request_counter": raw.request_counter,
            "response_counter": raw.response_counter,
            "session_status": {
                "resume": bool(response.sessionStatus.resume),
                "daq_running": bool(response.sessionStatus.daqRunning),
                "daq_config_lost": bool(response.sessionStatus.DaqCfgLost),
                "clear_daq_request": bool(response.sessionStatus.clearDaqRequest),
                "store_daq_request": bool(response.sessionStatus.storeDaqRequest),
                "calpag_config_lost": bool(response.sessionStatus.calPagCfgLost),
                "store_cal_request": bool(response.sessionStatus.storeCalRequest),
            },
            "resource_protection": {
                "calpag": bool(response.resourceProtectionStatus.calpag),
                "daq": bool(response.resourceProtectionStatus.daq),
                "stim": bool(response.resourceProtectionStatus.stim),
                "pgm": bool(response.resourceProtectionStatus.pgm),
                "dbg": bool(response.resourceProtectionStatus.dbg),
            },
            "state_number": int(response.stateNumber),
            "session_configuration": int(response.sessionConfiguration),
        }
        expected = {
            "raw_pdu": "ff0000000000",
            "request_counter": raw.request_counter,
            "response_counter": raw.response_counter,
            "session_status": {
                "resume": False,
                "daq_running": False,
                "daq_config_lost": False,
                "clear_daq_request": False,
                "store_daq_request": False,
                "calpag_config_lost": False,
                "store_cal_request": False,
            },
            "resource_protection": {"calpag": False, "daq": False, "stim": False, "pgm": False, "dbg": False},
            "state_number": 0,
            "session_configuration": 0,
        }
        if observed != expected:
            raise AssertionError(f"GET_STATUS profile mismatch: expected {expected!r}, got {observed!r}")
        return observed


def run_external_s0(
    port: str,
    capture: Capture,
    parsed: ParsedA2L,
    bundle_identity: BundleIdentity,
    config: SxiConfig,
) -> dict[str, Any]:
    """Run the bounded same-value workflow against a separately authorized XCP endpoint."""
    session = ClientSession(port, capture, config)
    checks: list[str] = []
    try:
        connect = assert_connect(session.master)
        checks.append("connect_exact_s0")
        status = assert_get_status(session.master)
        checks.append("get_status_exact_s0")
        identity = verify_bundle_identity(session.master, bundle_identity)
        checks.append("matched_build_schema_identity_before_download")

        period_meta = parsed.symbol("led.period_ms")
        duty_meta = parsed.symbol("led.duty_permille")
        revision_meta = parsed.symbol("led.active_revision")
        phase_meta = parsed.symbol("led.phase_us")
        period = read_scalar(session.master, period_meta)
        duty = read_scalar(session.master, duty_meta)
        capture.sample(period_meta.symbol, period, period_meta.unit, period_meta.address)
        capture.sample(duty_meta.symbol, duty, duty_meta.unit, duty_meta.address)
        revision = read_scalar(session.master, revision_meta)
        phase = read_scalar(session.master, phase_meta)
        write_scalar(session.master, period_meta, period, identity)
        if (read_scalar(session.master, revision_meta), read_scalar(session.master, phase_meta)) != (revision, phase):
            raise AssertionError("same-value write changed revision or phase")
        checks.append("metadata_driven_same_value_owner_reconciliation")
        expect_error(lambda: session.master.transport.request(types.Command.ALLOC_DAQ, 0, 1), 0x20)
        checks.append("daq_unsupported")
        if bytes(session.master.synch()) != b"\x00":
            raise AssertionError("SYNCH did not return ERR_CMD_SYNCH payload 00")
        checks.append("synch")
        session.master.disconnect()
        checks.append("disconnect")
        return {
            "status": "pass",
            "checks": checks,
            "connect": connect,
            "get_status": status,
            "verified_identity": asdict(identity),
        }
    finally:
        session.close()


def profile_dict(port: str, config: SxiConfig) -> dict[str, Any]:
    return {"port": port, **asdict(config)}


def verify_captured_frames(path: Path) -> None:
    """Ensure endpoint-side accepted raw frames obey the selected format."""
    decoder = StreamFrameDecoder()
    if not path.exists():
        return
    import json

    for line in path.read_text(encoding="utf-8").splitlines():
        entry = json.loads(line)
        if entry.get("status") == "accepted":
            decoder.feed(bytes.fromhex(entry["raw_frame_hex"]))
    if decoder.buffer:
        raise AssertionError("accepted capture ended in a partial frame")
