"""Immutable values selected by the reviewed S0 companion profile."""

from __future__ import annotations

from dataclasses import dataclass


PYXCP_VERSION = "0.29.18"
PYXCP_COMMIT = "016cf3e44364e9cd93966d144a39d342578a0391"
PYA2LDB_VERSION = "1.0.353"
PYA2L_COMMIT = "c19c3ad2f285d1230e334bad81eeb09cbaaa3031"
CONTRACT_RELEASE = "contracts-s0-v0.1.1"
CONTRACT_ARCHIVE_SHA256 = "f6f75d443d96dd762d3640e1acc76480243eabc88fd441574b4cad4f4f6705db"


@dataclass(frozen=True)
class SxiConfig:
    name: str
    scope: str
    qualification: str
    bitrate: int
    bytesize: int = 8
    parity: str = "N"
    stopbits: int = 1
    mode: str = "ASYNCH_FULL_DUPLEX_MODE"
    header_format: str = "HEADER_LEN_CTR_BYTE"
    tail_format: str = "CHECKSUM_BYTE"
    framing: bool = False
    alignment: int = 1
    timeout_seconds: float = 0.5
    recovery_quiet_seconds: float = 0.05
    target_inter_byte_timeout_seconds: float = 0.02


REFERENCE_PTY_SXI = SxiConfig(
    name="reference-pty-38400-8n1",
    scope="reference_pty",
    qualification="historical candidate used only for PTY harness qualification",
    bitrate=38_400,
)

EXTERNAL_19200_SXI = SxiConfig(
    name="reviewed-19200-8n1",
    scope="external_fixture",
    qualification=(
        "reviewed current fixture transport selection; physical XCP endpoint, "
        "serial side effects, and device execution remain unqualified"
    ),
    bitrate=19_200,
)

EXTERNAL_PROFILES = {EXTERNAL_19200_SXI.name: EXTERNAL_19200_SXI}

# S0 logical addresses. These are never interpreted as process pointers.
BUILD_ID = 0x0000_0000
SCHEMA_ID = 0x0000_0020
PROFILE_ID = 0x0000_0040
CAPABILITIES = 0x0000_0044
CLOCK_QUANTUM_US = 0x0000_0048
PERIOD_MS = 0x0000_1000
DUTY_PERMILLE = 0x0000_1002
UPTIME_MS = 0x0000_2000
SAMPLE_SEQUENCE = 0x0000_2004
ACTIVE_REVISION = 0x0000_2008
PHASE_US = 0x0000_200C
APPLIED_PERIOD_MS = 0x0000_2010
APPLIED_DUTY_PERMILLE = 0x0000_2012
COMMANDED_ON = 0x0000_2014
REJECTED_WRITE_COUNT = 0x0000_2018
DROPPED_SAMPLE_COUNT = 0x0000_201C
EDGE_LATENESS_US = 0x0000_2020
MAX_EDGE_LATENESS_US = 0x0000_2024
