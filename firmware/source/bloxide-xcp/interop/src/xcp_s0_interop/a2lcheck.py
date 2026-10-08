"""Typed independent pya2l model for the fixed S0 exchange fixture."""

from __future__ import annotations

import hashlib
import json
import tempfile
from dataclasses import dataclass
from pathlib import Path
from typing import Any

import pya2l
from pya2l import model as pya2l_model


TYPE_WIDTHS = {"UBYTE": 1, "UWORD": 2, "ULONG": 4}
TYPE_WIRE_NAMES = {"UBYTE": "u8", "UWORD": "u16", "ULONG": "u32"}
BYTE_ORDERS = {"MSB_LAST": "little", "MSB_FIRST": "big"}


@dataclass(frozen=True)
class ScalarMetadata:
    symbol: str
    kind: str
    address: int
    address_extension: int
    a2l_type: str
    wire_type: str
    width: int
    byte_order: str
    unit: str
    minimum: int
    maximum: int
    access: str
    default: int | None

    def decode(self, data: bytes) -> int:
        if len(data) != self.width:
            raise ValueError(f"{self.symbol} requires {self.width} bytes, got {len(data)}")
        return int.from_bytes(data, self.byte_order)

    def encode(self, value: int) -> bytes:
        if not self.minimum <= value <= self.maximum:
            raise ValueError(f"{self.symbol} value {value} outside {self.minimum}..{self.maximum} {self.unit}")
        return value.to_bytes(self.width, self.byte_order)

    def to_result(self) -> dict[str, Any]:
        return {
            "kind": self.kind,
            "address": self.address,
            "address_extension": self.address_extension,
            "a2l_type": self.a2l_type,
            "wire_type": self.wire_type,
            "width": self.width,
            "byte_order": self.byte_order,
            "unit": self.unit,
            "minimum": self.minimum,
            "maximum": self.maximum,
            "access": self.access,
            "default": self.default,
        }


@dataclass(frozen=True)
class ParsedA2L:
    byte_order: str
    symbols: dict[str, ScalarMetadata]
    a2l_sha256: str
    descriptors_sha256: str

    def symbol(self, name: str) -> ScalarMetadata:
        try:
            return self.symbols[name]
        except KeyError as exc:
            raise KeyError(f"required parsed A2L symbol is missing: {name}") from exc

    def to_result(self) -> dict[str, Any]:
        return {
            "status": "pass",
            "api": "pya2l.import_a2l",
            "symbol_count": len(self.symbols),
            "byte_order": self.byte_order,
            "symbols": {name: item.to_result() for name, item in self.symbols.items()},
            "a2l_sha256": self.a2l_sha256,
            "descriptors_sha256": self.descriptors_sha256,
        }


def parse_a2l(a2l_path: Path, descriptors_path: Path) -> ParsedA2L:
    descriptors = json.loads(descriptors_path.read_text(encoding="utf-8"))
    expected = {item["symbol"]: item for item in descriptors["variables"]}
    observed: dict[str, dict[str, Any]] = {}
    with tempfile.TemporaryDirectory(prefix="bloxide-xcp-a2l-") as output_dir:
        session = pya2l.import_a2l(
            str(a2l_path),
            output_dir=output_dir,
            force_overwrite=True,
            progress_bar=False,
            loglevel="ERROR",
        )
        try:
            methods = {item.name: item.unit for item in session.query(pya2l_model.CompuMethod).all()}
            mod_common = session.query(pya2l_model.ModCommon).one()
            a2l_order = mod_common.byte_order.byteOrder
            try:
                byte_order = BYTE_ORDERS[a2l_order]
            except KeyError as exc:
                raise AssertionError(f"unsupported A2L byte order {a2l_order!r}") from exc
            for item in session.query(pya2l_model.Characteristic).all():
                a2l_type = session.query(pya2l_model.RecordLayout).filter_by(name=item.deposit).one().fnc_values.datatype
                observed[item.name] = {
                    "kind": "CHARACTERISTIC",
                    "address": int(item.address),
                    "a2l_type": a2l_type,
                    "unit": methods[item.conversion],
                    "minimum": int(item.lowerLimit),
                    "maximum": int(item.upperLimit),
                    "address_extension": int(item.ecu_address_extension.extension),
                    "access": "read_only" if item.read_only else "calibration_read_write",
                }
            for item in session.query(pya2l_model.Measurement).all():
                observed[item.name] = {
                    "kind": "MEASUREMENT",
                    "address": int(item.ecu_address.address),
                    "a2l_type": item.datatype,
                    "unit": methods[item.conversion],
                    "minimum": int(item.lowerLimit),
                    "maximum": int(item.upperLimit),
                    "address_extension": int(item.ecu_address_extension.extension),
                    "access": "read_only",
                }
        finally:
            session.close()

    if descriptors.get("byte_order") != byte_order:
        raise AssertionError(
            f"descriptor byte order {descriptors.get('byte_order')!r} differs from parsed A2L {byte_order!r}"
        )
    if set(observed) != set(expected):
        raise AssertionError(f"A2L symbol set differs: expected={sorted(expected)}, observed={sorted(observed)}")

    symbols: dict[str, ScalarMetadata] = {}
    for symbol, descriptor in expected.items():
        item = observed[symbol]
        for key in ("kind", "address", "a2l_type", "unit", "minimum", "maximum", "access"):
            if item[key] != descriptor[key]:
                raise AssertionError(f"{symbol} {key}: expected {descriptor[key]!r}, observed {item[key]!r}")
        expected_extension = int(descriptor.get("address_extension", descriptors.get("address_extension", 0)))
        if item["address_extension"] != expected_extension:
            raise AssertionError(
                f"{symbol} address_extension: expected {expected_extension}, observed {item['address_extension']}"
            )
        a2l_type = item["a2l_type"]
        if a2l_type not in TYPE_WIDTHS:
            raise AssertionError(f"{symbol} uses unsupported A2L scalar type {a2l_type}")
        if descriptor["wire_type"] != TYPE_WIRE_NAMES[a2l_type]:
            raise AssertionError(
                f"{symbol} wire type {descriptor['wire_type']!r} conflicts with parsed {a2l_type}"
            )
        symbols[symbol] = ScalarMetadata(
            symbol=symbol,
            kind=item["kind"],
            address=item["address"],
            address_extension=item["address_extension"],
            a2l_type=a2l_type,
            wire_type=descriptor["wire_type"],
            width=TYPE_WIDTHS[a2l_type],
            byte_order=byte_order,
            unit=item["unit"],
            minimum=item["minimum"],
            maximum=item["maximum"],
            access=item["access"],
            default=descriptor.get("default"),
        )
    return ParsedA2L(
        byte_order=byte_order,
        symbols=symbols,
        a2l_sha256=hashlib.sha256(a2l_path.read_bytes()).hexdigest(),
        descriptors_sha256=hashlib.sha256(descriptors_path.read_bytes()).hexdigest(),
    )


def validate_a2l(a2l_path: Path, descriptors_path: Path) -> dict[str, Any]:
    return parse_a2l(a2l_path, descriptors_path).to_result()
