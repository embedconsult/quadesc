import json
from dataclasses import asdict
from pathlib import Path

from xcp_s0_interop.bundle import load_bundle_identity
from xcp_s0_interop.profile import (
    EXTERNAL_19200_SXI,
    PYA2L_COMMIT,
    PYA2LDB_VERSION,
    PYXCP_COMMIT,
    PYXCP_VERSION,
    REFERENCE_PTY_SXI,
)


FIXTURES = Path(__file__).parents[1] / "fixtures"


def test_exact_selected_dependency_pins():
    assert (PYXCP_VERSION, PYXCP_COMMIT) == (
        "0.29.18",
        "016cf3e44364e9cd93966d144a39d342578a0391",
    )
    assert (PYA2LDB_VERSION, PYA2L_COMMIT) == (
        "1.0.353",
        "c19c3ad2f285d1230e334bad81eeb09cbaaa3031",
    )


def test_companion_configuration_is_exact():
    profile = json.loads((FIXTURES / "transport-profile.json").read_text())
    expected = asdict(REFERENCE_PTY_SXI)
    assert profile["scope"] == "reference_pty"
    assert profile["connection_profile"] == REFERENCE_PTY_SXI.name
    for key in (
        "bitrate",
        "bytesize",
        "parity",
        "stopbits",
        "mode",
        "header_format",
        "tail_format",
        "framing",
        "alignment",
        "timeout_seconds",
    ):
        assert profile["connection"][key] == expected[key]


def test_external_profile_is_separate_reviewed_19200_8n1():
    profile = json.loads((FIXTURES / "external-transport-profile.json").read_text())
    assert profile["scope"] == "external_fixture"
    assert profile["connection_profile"] == EXTERNAL_19200_SXI.name
    assert profile["connection"]["bitrate"] == EXTERNAL_19200_SXI.bitrate == 19_200
    assert (EXTERNAL_19200_SXI.bytesize, EXTERNAL_19200_SXI.parity, EXTERNAL_19200_SXI.stopbits) == (8, "N", 1)
    assert "diagnostic protocol, not an XCP endpoint" in profile["warning"]


def test_companion_identity_uses_frozen_s0_addresses_and_widths():
    identity = load_bundle_identity(FIXTURES / "bundle-metadata.json")
    assert (identity.build_id.address, len(identity.build_id.value)) == (0x0000, 32)
    assert (identity.schema_id.address, len(identity.schema_id.value)) == (0x0020, 32)
