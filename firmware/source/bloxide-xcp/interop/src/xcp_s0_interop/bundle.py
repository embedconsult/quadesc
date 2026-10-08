"""Companion-bundle identity metadata for matched S0 client operation."""

from __future__ import annotations

import json
from dataclasses import dataclass
from pathlib import Path

from .profile import BUILD_ID, SCHEMA_ID


@dataclass(frozen=True)
class IdentityBlob:
    name: str
    address: int
    value: bytes


@dataclass(frozen=True)
class BundleIdentity:
    build_id: IdentityBlob
    schema_id: IdentityBlob

    def blobs(self) -> tuple[IdentityBlob, IdentityBlob]:
        return (self.build_id, self.schema_id)

    def to_result(self) -> dict[str, dict[str, int | str]]:
        return {
            blob.name: {
                "address": blob.address,
                "size": len(blob.value),
                "expected_hex": blob.value.hex(),
            }
            for blob in self.blobs()
        }


def load_bundle_identity(path: Path) -> BundleIdentity:
    data = json.loads(path.read_text(encoding="utf-8"))

    def load(name: str, required_address: int) -> IdentityBlob:
        item = data["identity"][name]
        address = int(item["address"])
        value = bytes.fromhex(item["value_hex"])
        if address != required_address:
            raise AssertionError(
                f"{name} address must use frozen S0 address 0x{required_address:08x}, got 0x{address:08x}"
            )
        if int(item["size"]) != 32 or len(value) != 32:
            raise AssertionError(f"{name} must be an exact 32-byte identity blob")
        return IdentityBlob(name=name, address=address, value=value)

    if data.get("profile") != "S0":
        raise AssertionError("companion identity metadata must select S0")
    return BundleIdentity(
        build_id=load("build_id", BUILD_ID),
        schema_id=load("schema_id", SCHEMA_ID),
    )
