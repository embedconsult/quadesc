"""End-to-end metadata-driven scenarios against the independent reference endpoint."""

from __future__ import annotations

import json
import time
from pathlib import Path
from typing import Callable

import serial
from pyxcp import types

from .a2lcheck import ParsedA2L, ScalarMetadata
from .bundle import BundleIdentity
from .capture import Capture
from .client import (
    ClientSession,
    assert_connect,
    assert_get_status,
    expect_error,
    read_scalar,
    verify_bundle_identity,
    write_scalar,
)
from .framing import encode_frame
from .profile import REFERENCE_PTY_SXI
from .simulator import ReferenceEndpoint


def _new_connected(port: str, capture: Capture) -> tuple[ClientSession, dict]:
    session = ClientSession(port, capture, REFERENCE_PTY_SXI)
    try:
        connect = assert_connect(session.master)
        return session, connect
    except Exception:
        session.close()
        raise


def _inject_bad_frames(port: str, endpoint: ReferenceEndpoint) -> None:
    config = REFERENCE_PTY_SXI
    raw = serial.Serial(port, config.bitrate, timeout=0.1, write_timeout=config.timeout_seconds)
    try:
        bad_checksum = bytearray(encode_frame(b"\xfd", 0xA0))
        bad_checksum[-1] ^= 0x01
        previous = endpoint.received_sequence()
        raw.write(bad_checksum)
        raw.flush()
        endpoint.wait_for_rx_after(previous)
        time.sleep(config.recovery_quiet_seconds + 0.02)
        previous = endpoint.received_sequence()
        raw.write(bytes.fromhex("02 a1 ff"))  # incomplete CONNECT frame
        raw.flush()
        endpoint.wait_for_rx_after(previous)
        time.sleep(config.recovery_quiet_seconds + 0.02)
        endpoint.wait_for_idle_recovery()
    finally:
        raw.close()


def _state(master, parsed: ParsedA2L) -> tuple[int, int, int]:
    return (
        read_scalar(master, parsed.symbol("led.period_ms")),
        read_scalar(master, parsed.symbol("led.duty_permille")),
        read_scalar(master, parsed.symbol("led.active_revision")),
    )


def run_reference_suite(
    capture: Capture,
    parsed: ParsedA2L,
    bundle_identity: BundleIdentity,
    *,
    endpoint_kwargs: dict | None = None,
) -> dict:
    endpoint = ReferenceEndpoint(capture, **(endpoint_kwargs or {})).start()
    checks: list[str] = []
    negative_checks: list[dict[str, str | int]] = []
    session: ClientSession | None = None
    try:
        session, connect = _new_connected(str(endpoint.slave_path), capture)
        master = session.master
        checks.append("connect_exact_s0")
        status = assert_get_status(master)
        checks.append("get_status_exact_s0")
        identity = verify_bundle_identity(master, bundle_identity)
        checks.append("matched_build_schema_identity_before_download")

        period = parsed.symbol("led.period_ms")
        duty = parsed.symbol("led.duty_permille")
        revision = parsed.symbol("led.active_revision")
        phase = parsed.symbol("led.phase_us")
        if read_scalar(master, period) != period.default or read_scalar(master, duty) != duty.default:
            raise AssertionError("reference defaults do not match companion descriptors")
        capture.sample(period.symbol, period.default, period.unit, period.address)
        capture.sample(duty.symbol, duty.default, duty.unit, duty.address)
        checks.append("metadata_driven_defaults_with_units")

        def assert_failed_transfer(
            name: str,
            set_initial_mta: Callable[[], None],
            operation: Callable[[], object],
            expected_error: int,
            observe_mta: Callable[[], str],
        ) -> None:
            before = _state(master, parsed)
            set_initial_mta()
            expect_error(operation, expected_error)
            mta_method = observe_mta()
            after = _state(master, parsed)
            if after != before:
                raise AssertionError(f"{name} changed calibration values or active revision: {before} -> {after}")
            item = {"name": name, "error": f"0x{expected_error:02x}", "mta_observation": mta_method}
            negative_checks.append(item)
            capture.record("decoded-events.jsonl", direction="assertion", behavior="negative_invariants", **item)

        def exact_scalar_probe(scalar: ScalarMetadata, expected_value: int) -> Callable[[], str]:
            expected = scalar.encode(expected_value)

            def observe() -> str:
                data = bytes(master.upload(scalar.width))
                if data != expected:
                    raise AssertionError(f"retained MTA read expected {expected.hex()}, got {data.hex()}")
                return "UPLOAD_without_SET_MTA"

            return observe

        def direct_mta(expected: int) -> Callable[[], str]:
            def observe() -> str:
                actual = endpoint.observed_mta()
                if actual != expected:
                    raise AssertionError(f"reference-observed MTA expected 0x{expected:08x}, got 0x{actual:08x}")
                return "reference_endpoint_state_for_unreadable_space"

            return observe

        period_value = read_scalar(master, period)
        for width, payload in ((0, b""), (1, b"\x00"), (3, b"\x00" * 3), (4, b"\x00" * 4)):
            assert_failed_transfer(
                f"download_width_{width}",
                lambda: master.setMta(period.address, period.address_extension),
                lambda width=width, payload=payload: master.transport.request(types.Command.DOWNLOAD, width, *payload),
                0x22,
                exact_scalar_probe(period, period_value),
            )

        for name, malformed in (
            ("download_missing_count", b"\xf0"),
            ("download_count_payload_short", b"\xf0\x02\x00"),
            ("download_count_payload_long", b"\xf0\x01\x00\x00"),
        ):
            assert_failed_transfer(
                name,
                lambda: master.setMta(period.address, period.address_extension),
                lambda malformed=malformed: master.transport.request(types.Command.DOWNLOAD, *malformed[1:]),
                0x21,
                exact_scalar_probe(period, period_value),
            )

        for value in (period.minimum - 1, period.maximum + 1):
            payload = value.to_bytes(period.width, period.byte_order, signed=False)
            assert_failed_transfer(
                f"period_range_{value}",
                lambda: master.setMta(period.address, period.address_extension),
                lambda payload=payload: master.transport.request(types.Command.DOWNLOAD, period.width, *payload),
                0x22,
                exact_scalar_probe(period, period_value),
            )

        assert_failed_transfer(
            "read_only_write",
            lambda: master.setMta(revision.address, revision.address_extension),
            lambda: master.download(bytes(2)),
            0x23,
            exact_scalar_probe(revision, read_scalar(master, revision)),
        )

        period_high = period.encode(period_value)[1:2]
        for name, operation in (
            ("cross_region_download", lambda: master.download(b"\x00\x00")),
            ("cross_region_upload", lambda: master.upload(3)),
        ):
            assert_failed_transfer(
                name,
                lambda: master.setMta(period.address + 1, period.address_extension),
                operation,
                0x24,
                lambda expected=period_high: (
                    "UPLOAD_without_SET_MTA"
                    if bytes(master.upload(1)) == expected
                    else (_ for _ in ()).throw(AssertionError("cross-region failure did not retain MTA"))
                ),
            )

        unmapped = 0x0000_3000
        for name, operation in (
            ("unmapped_download", lambda: master.download(b"\x00\x00")),
            ("unmapped_upload", lambda: master.upload(1)),
        ):
            assert_failed_transfer(
                name,
                lambda: master.setMta(unmapped, 0),
                operation,
                0x24,
                direct_mta(unmapped),
            )

        set_mta_bytes = tuple(period.address.to_bytes(4, "little"))
        assert_failed_transfer(
            "set_mta_bad_extension",
            lambda: master.setMta(period.address, period.address_extension),
            lambda: master.transport.request(types.Command.SET_MTA, 0, 0, 1, *set_mta_bytes),
            0x22,
            exact_scalar_probe(period, period_value),
        )
        assert_failed_transfer(
            "set_mta_reserved_nonzero",
            lambda: master.setMta(period.address, period.address_extension),
            lambda: master.transport.request(types.Command.SET_MTA, 1, 0, 0, *set_mta_bytes),
            0x21,
            exact_scalar_probe(period, period_value),
        )

        overflow = 0xFFFF_FFFF
        for name, operation in (
            ("overflow_download", lambda: master.download(b"\x00\x00")),
            ("overflow_upload", lambda: master.upload(2)),
        ):
            assert_failed_transfer(
                name,
                lambda: master.setMta(overflow, 0),
                operation,
                0x22,
                direct_mta(overflow),
            )
        checks.append("negative_exact_errors_values_revision_and_mta")

        for value in (period.minimum, period.maximum):
            write_scalar(master, period, value, identity)
            if read_scalar(master, period) != value:
                raise AssertionError(f"period boundary {value} did not read back")
        for value in (duty.minimum, 1, duty.maximum - 1, duty.maximum):
            write_scalar(master, duty, value, identity)
            if read_scalar(master, duty) != value:
                raise AssertionError(f"duty boundary {value} did not read back")
        checks.append("metadata_driven_scalar_success_boundaries")

        expect_error(lambda: master.transport.request(types.Command.ALLOC_DAQ, 0, 1), 0x20)
        checks.append("daq_unsupported")
        if bytes(master.synch()) != b"\x00":
            raise AssertionError("SYNCH response was not ERR_CMD_SYNCH")
        checks.append("synch_defined_error")

        write_scalar(master, period, period.default, identity)
        revision_before = read_scalar(master, revision)
        endpoint.suppress_next_download_response = True
        master.setMta(period.address, period.address_extension)
        try:
            master.download(period.encode(321))
        except types.XcpTimeoutError:
            pass
        else:
            raise AssertionError("lost-response injection did not time out")
        session.close()
        session = None
        time.sleep(REFERENCE_PTY_SXI.recovery_quiet_seconds + 0.02)
        session, _ = _new_connected(str(endpoint.slave_path), capture)
        master = session.master
        assert_get_status(master)
        identity = verify_bundle_identity(master, bundle_identity)
        revision_after = read_scalar(master, revision)
        phase_after = read_scalar(master, phase)
        if read_scalar(master, period) != 321 or revision_after != (revision_before + 1) & 0xFFFF_FFFF:
            raise AssertionError("reconnect/readback did not reconcile the committed unknown outcome")
        write_scalar(master, period, 321, identity)
        if (read_scalar(master, revision), read_scalar(master, phase)) != (revision_after, phase_after):
            raise AssertionError("same-value retry changed revision or phase")
        checks.append("truthful_owner_reconciliation_and_same_value")

        master.disconnect()
        session.close()
        session = None
        # A successful DISCONNECT ends the owned client transport. Probe the
        # disconnected target independently; do not revive that client object.
        with serial.Serial(
            str(endpoint.slave_path), REFERENCE_PTY_SXI.bitrate,
            timeout=REFERENCE_PTY_SXI.timeout_seconds,
            write_timeout=REFERENCE_PTY_SXI.timeout_seconds,
        ) as raw:
            request = encode_frame(b"\xfd", 0xA2)
            capture.record(
                "host-wire.jsonl", transport="uart", direction="host_to_endpoint",
                raw_chunk_hex=request.hex(), octets=len(request), scope="disconnected_target_probe",
            )
            raw.write(request)
            raw.flush()
            response = raw.read(1)
            if response:
                capture.record(
                    "host-wire.jsonl", transport="uart", direction="endpoint_to_host",
                    raw_chunk_hex=response.hex(), octets=len(response), scope="disconnected_target_probe",
                )
                raise AssertionError("disconnected endpoint responded to GET_STATUS")
        checks.append("disconnect_ignores_non_connect")

        _inject_bad_frames(str(endpoint.slave_path), endpoint)
        session, _ = _new_connected(str(endpoint.slave_path), capture)
        assert_get_status(session.master)
        verify_bundle_identity(session.master, bundle_identity)
        if read_scalar(session.master, period) != 321:
            raise AssertionError("state/readback not recovered after framing errors")
        checks.append("checksum_partial_frame_idle_recovery")
        session.master.disconnect()

        return {
            "status": "pass",
            "endpoint": "documented_reference_simulator",
            "transport": "pty_sxi_uart",
            "port": str(endpoint.slave_path),
            "checks": checks,
            "negative_checks": negative_checks,
            "connect": connect,
            "get_status": status,
            "verified_identity": {"build_id": identity.build_id, "schema_id": identity.schema_id},
            "can": {"status": "not_run", "reason": "S0 T10 PTY path; CAN binding/qualification is a later gate"},
            "claims": {
                "scoped_s0_interoperability": True,
                "full_asam_conformity": False,
                "physical_uart": False,
                "gpio_or_optical_completion": False,
            },
        }
    finally:
        if session is not None:
            session.close()
        endpoint.close()


def load_jsonl(path: Path) -> list[dict]:
    return [json.loads(line) for line in path.read_text(encoding="utf-8").splitlines()]
