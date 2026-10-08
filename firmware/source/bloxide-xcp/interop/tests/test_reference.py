import json
from pathlib import Path

import pytest
from construct import ConstructError

from xcp_s0_interop.a2lcheck import parse_a2l
from xcp_s0_interop.bundle import load_bundle_identity
from xcp_s0_interop.capture import Capture
from xcp_s0_interop.client import (
    ClientSession,
    IdentityMismatchError,
    assert_connect,
    assert_get_status,
    read_scalar,
    verify_bundle_identity,
    write_scalar,
)
from xcp_s0_interop.cli import main
from xcp_s0_interop.framing import decode_frame
from xcp_s0_interop.profile import REFERENCE_PTY_SXI
from xcp_s0_interop.scenario import load_jsonl, run_reference_suite
from xcp_s0_interop.simulator import ReferenceEndpoint


FIXTURES = Path(__file__).parents[1] / "fixtures"


def fixture_model():
    return parse_a2l(FIXTURES / "led-s0.a2l", FIXTURES / "descriptors.json")


def fixture_identity():
    return load_bundle_identity(FIXTURES / "bundle-metadata.json")


def accepted_host_pdus(path: Path) -> list[bytes]:
    return [
        decode_frame(bytes.fromhex(item["raw_frame_hex"]))[1]
        for item in load_jsonl(path)
        if item["direction"] == "host_to_endpoint" and item["status"] == "accepted"
    ]


def test_full_reference_suite_and_evidence(tmp_path):
    root = tmp_path / "reference-run"
    capture = Capture(root)
    result = run_reference_suite(capture, fixture_model(), fixture_identity())
    capture.write_json("result.json", result)
    hashes = capture.finalize_manifest()

    assert result["status"] == "pass"
    assert result["connect"]["raw_pdu"] == "ff01000808000101"
    assert result["get_status"]["raw_pdu"] == "ff0000000000"
    assert result["claims"]["full_asam_conformity"] is False
    assert result["can"]["status"] == "not_run"
    assert "truthful_owner_reconciliation_and_same_value" in result["checks"]
    assert "checksum_partial_frame_idle_recovery" in result["checks"]
    assert {item["name"] for item in result["negative_checks"]} >= {
        "download_width_0",
        "download_width_1",
        "download_width_3",
        "download_width_4",
        "set_mta_bad_extension",
        "set_mta_reserved_nonzero",
        "overflow_download",
        "overflow_upload",
        "unmapped_download",
        "unmapped_upload",
        "cross_region_download",
        "cross_region_upload",
    }
    raw = load_jsonl(root / "raw-uart.jsonl")
    assert any(item["status"] == "rejected" for item in raw)
    assert any(item["status"] == "discarded" for item in raw)
    assert {item["direction"] for item in raw} == {"host_to_endpoint", "endpoint_to_host"}
    samples = load_jsonl(root / "decoded-samples.jsonl")
    assert {(item["symbol"], item["unit"]) for item in samples} == {
        ("led.period_ms", "ms"),
        ("led.duty_permille", "permille"),
    }
    assert set(hashes) >= {"raw-uart.jsonl", "decoded-events.jsonl", "decoded-samples.jsonl", "result.json"}

    pdus = accepted_host_pdus(root / "raw-uart.jsonl")
    first_download = next(index for index, pdu in enumerate(pdus) if pdu[0] == 0xF0)
    mta_before_download = [int.from_bytes(pdu[4:8], "little") for pdu in pdus[:first_download] if pdu[0] == 0xF6]
    assert 0x0000 in mta_before_download
    assert 0x0020 in mta_before_download
    assert sorted({pdu[1] for pdu in pdus if pdu[0] == 0xF0 and len(pdu) > 1}) == [0, 1, 2, 3, 4]
    assert bytes.fromhex("f0") in pdus
    assert bytes.fromhex("f00200") in pdus
    assert bytes.fromhex("f0010000") in pdus


def test_parsed_moved_address_controls_raw_wire_operations(tmp_path):
    a2l_path = tmp_path / "moved.a2l"
    descriptors_path = tmp_path / "moved-descriptors.json"
    a2l_path.write_text(
        (FIXTURES / "led-s0.a2l").read_text(encoding="utf-8").replace(
            'led.period_ms "LED period" VALUE 0x1000', 'led.period_ms "LED period" VALUE 0x1100'
        ),
        encoding="utf-8",
    )
    descriptors = json.loads((FIXTURES / "descriptors.json").read_text(encoding="utf-8"))
    next(item for item in descriptors["variables"] if item["symbol"] == "led.period_ms")["address"] = 0x1100
    descriptors_path.write_text(json.dumps(descriptors), encoding="utf-8")
    parsed = parse_a2l(a2l_path, descriptors_path)

    root = tmp_path / "moved-run"
    capture = Capture(root)
    endpoint = ReferenceEndpoint(capture, period_address=0x1100).start()
    session = ClientSession(str(endpoint.slave_path), capture, REFERENCE_PTY_SXI)
    try:
        assert_connect(session.master)
        assert_get_status(session.master)
        verified = verify_bundle_identity(session.master, fixture_identity())
        period = parsed.symbol("led.period_ms")
        assert read_scalar(session.master, period) == 1000
        write_scalar(session.master, period, 1000, verified)
    finally:
        session.close()
        endpoint.close()

    pdus = accepted_host_pdus(root / "raw-uart.jsonl")
    scalar_mtas = [int.from_bytes(pdu[4:8], "little") for pdu in pdus if pdu[0] == 0xF6]
    assert 0x1100 in scalar_mtas
    assert 0x1000 not in scalar_mtas
    assert bytes.fromhex("f002e803") in pdus


@pytest.mark.parametrize("changed", ["build", "schema"])
def test_identity_mismatch_fails_closed_before_download(tmp_path, changed):
    root = tmp_path / f"identity-{changed}"
    capture = Capture(root)
    kwargs = {"build_id": bytes.fromhex("ee" * 32)} if changed == "build" else {"schema_id": bytes.fromhex("ee" * 32)}
    endpoint = ReferenceEndpoint(capture, **kwargs).start()
    session = ClientSession(str(endpoint.slave_path), capture, REFERENCE_PTY_SXI)
    try:
        assert_connect(session.master)
        assert_get_status(session.master)
        with pytest.raises(IdentityMismatchError, match=f"{changed}_id mismatch"):
            verify_bundle_identity(session.master, fixture_identity())
    finally:
        session.close()
        endpoint.close()
    pdus = accepted_host_pdus(root / "raw-uart.jsonl")
    assert not any(pdu[0] == 0xF0 for pdu in pdus)
    identity_mtas = [int.from_bytes(pdu[4:8], "little") for pdu in pdus if pdu[0] == 0xF6]
    assert 0x0000 in identity_mtas and 0x0020 in identity_mtas


def test_external_cli_workflow_is_qualified_against_pty_only(tmp_path):
    oracle_root = tmp_path / "external-pty-oracle"
    endpoint = ReferenceEndpoint(Capture(oracle_root)).start()
    output = tmp_path / "external-cli-run"
    try:
        assert (
            main(
                [
                    "endpoint",
                    "--port",
                    str(endpoint.slave_path),
                    "--connection-profile",
                    "reviewed-19200-8n1",
                    "--output",
                    str(output),
                ]
            )
            == 0
        )
    finally:
        endpoint.close()
    result = json.loads((output / "result.json").read_text(encoding="utf-8"))
    assert result["status"] == "pass"
    assert result["endpoint"] == "external"
    assert result["profile"]["scope"] == "external_fixture"
    assert result["profile"]["bitrate"] == 19_200
    assert "diagnostic, not an XCP endpoint" in result["warnings"][0]
    assert any(pdu[0] == 0xF0 for pdu in accepted_host_pdus(oracle_root / "raw-uart.jsonl"))


CONNECT_MUTATIONS = [
    ("missing_calpag", 1, 0x00),
    ("resource_reserved_bit_1", 1, 0x03),
    ("resource_reserved_bit_6", 1, 0x41),
    ("resource_reserved_bit_7", 1, 0x81),
    ("advertise_daq", 1, 0x05),
    ("advertise_stim", 1, 0x09),
    ("advertise_pgm", 1, 0x11),
    ("advertise_dbg", 1, 0x21),
    ("wrong_byte_order", 2, 0x01),
    ("word_granularity", 2, 0x02),
    ("slave_block_mode", 2, 0x40),
    ("optional_comm_info", 2, 0x80),
    ("max_cto", 3, 0x09),
    ("max_dto", 4, 0x09),
    ("protocol_version", 6, 0x02),
    ("transport_version", 7, 0x02),
]


@pytest.mark.parametrize(("name", "offset", "value"), CONNECT_MUTATIONS, ids=[item[0] for item in CONNECT_MUTATIONS])
def test_connect_capability_mutations_are_rejected(tmp_path, name, offset, value):
    response = bytearray.fromhex("ff01000808000101")
    response[offset] = value
    capture = Capture(tmp_path / name)
    endpoint = ReferenceEndpoint(capture, connect_response=bytes(response)).start()
    session = ClientSession(str(endpoint.slave_path), capture, REFERENCE_PTY_SXI)
    try:
        with pytest.raises(AssertionError, match=r"CONNECT (raw response|profile) mismatch"):
            assert_connect(session.master)
    finally:
        session.close()
        endpoint.close()
    requests = accepted_host_pdus(capture.root / "raw-uart.jsonl")
    assert not any(pdu[0] in (0xF6, 0xF0) for pdu in requests)


STATUS_MUTATIONS = [
    ("session_status", 1, 0x7F),
    ("resource_protection_reserved_bit_1", 2, 0x02),
    ("resource_protection_reserved_bit_6", 2, 0x40),
    ("resource_protection_reserved_bit_7", 2, 0x80),
    ("resource_protection", 2, 0x3D),
    ("state_number", 3, 0x01),
    ("session_configuration_low", 4, 0x01),
    ("session_configuration_high", 5, 0x01),
]


@pytest.mark.parametrize(("name", "offset", "value"), STATUS_MUTATIONS, ids=[item[0] for item in STATUS_MUTATIONS])
def test_status_mutations_are_rejected(tmp_path, name, offset, value):
    response = bytearray.fromhex("ff0000000000")
    response[offset] = value
    capture = Capture(tmp_path / name)
    endpoint = ReferenceEndpoint(capture, status_response=bytes(response)).start()
    session = ClientSession(str(endpoint.slave_path), capture, REFERENCE_PTY_SXI)
    try:
        assert_connect(session.master)
        with pytest.raises(AssertionError, match=r"GET_STATUS (raw response|profile) mismatch"):
            assert_get_status(session.master)
    finally:
        session.close()
        endpoint.close()
    requests = accepted_host_pdus(capture.root / "raw-uart.jsonl")
    assert not any(pdu[0] in (0xF6, 0xF0) for pdu in requests)


def test_capture_refuses_existing_output_without_overwrite(tmp_path):
    root = tmp_path / "one-run"
    first = Capture(root)
    first.write_json("marker.json", {"run": 1})
    before = (root / "marker.json").read_bytes()
    with pytest.raises(FileExistsError, match="must be fresh"):
        Capture(root)
    assert (root / "marker.json").read_bytes() == before


@pytest.mark.parametrize(
    ("command", "response_name", "response"),
    [
        ("connect", "connect_short", bytes.fromhex("ff010008080001")),
        ("status", "status_short", bytes.fromhex("ff00000000")),
        ("status", "status_long", bytes.fromhex("ff000000000000")),
    ],
)
def test_exact_response_lengths_are_rejected_before_identity_or_download(tmp_path, command, response_name, response):
    capture = Capture(tmp_path / response_name)
    kwargs = {"connect_response": response} if command == "connect" else {"status_response": response}
    endpoint = ReferenceEndpoint(capture, **kwargs).start()
    session = ClientSession(str(endpoint.slave_path), capture, REFERENCE_PTY_SXI)
    try:
        if command == "status":
            assert_connect(session.master)
        with pytest.raises((AssertionError, ConstructError)):
            (assert_connect if command == "connect" else assert_get_status)(session.master)
    finally:
        session.close()
        endpoint.close()
    requests = accepted_host_pdus(capture.root / "raw-uart.jsonl")
    assert not any(pdu[0] in (0xF6, 0xF0) for pdu in requests)
