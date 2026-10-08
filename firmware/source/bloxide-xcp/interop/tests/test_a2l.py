from pathlib import Path

from xcp_s0_interop.a2lcheck import parse_a2l, validate_a2l


FIXTURES = Path(__file__).parents[1] / "fixtures"


def test_pinned_parser_import_and_descriptor_comparison():
    result = validate_a2l(FIXTURES / "led-s0.a2l", FIXTURES / "descriptors.json")
    assert result["status"] == "pass"
    assert result["api"] == "pya2l.import_a2l"
    assert result["symbol_count"] == 13
    assert result["byte_order"] == "little"
    assert result["symbols"]["led.period_ms"]["width"] == 2


def test_parser_returns_typed_operation_metadata():
    parsed = parse_a2l(FIXTURES / "led-s0.a2l", FIXTURES / "descriptors.json")
    period = parsed.symbol("led.period_ms")
    assert (period.address, period.address_extension) == (0x1000, 0)
    assert (period.a2l_type, period.wire_type, period.width, period.byte_order) == ("UWORD", "u16", 2, "little")
    assert (period.minimum, period.maximum, period.unit, period.access) == (
        100,
        10_000,
        "ms",
        "calibration_read_write",
    )
    assert period.decode(period.encode(321)) == 321
