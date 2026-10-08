"""T34 versioned transport selection stays aligned with the accepted profiles."""

import json
from pathlib import Path

from xcp_s0_interop.profile import EXTERNAL_19200_SXI, REFERENCE_PTY_SXI


def test_product_transport_profiles_are_explicit_and_unescaped():
    root = Path(__file__).parents[2]
    product = json.loads((root / "config" / "sxi-transport-profiles.json").read_text())
    assert product["selected_external_profile"] == "reviewed-19200-8n1"
    external = product["profiles"]["reviewed-19200-8n1"]
    historical = product["profiles"]["reference-pty-38400-8n1"]
    assert (external["bitrate"], external["bytesize"], external["parity"], external["stopbits"]) == (
        EXTERNAL_19200_SXI.bitrate,
        EXTERNAL_19200_SXI.bytesize,
        EXTERNAL_19200_SXI.parity,
        EXTERNAL_19200_SXI.stopbits,
    )
    assert historical["bitrate"] == REFERENCE_PTY_SXI.bitrate
    for profile in (external, historical):
        assert profile["header_format"] == "HEADER_LEN_CTR_BYTE"
        assert profile["tail_format"] == "CHECKSUM_BYTE"
        assert profile["framing"] is False
        assert profile["inter_byte_timeout_ms"] == 20
        assert profile["host_reopen_quiet_ms"] == 50
        assert profile["physical_uart_qualified"] is False
