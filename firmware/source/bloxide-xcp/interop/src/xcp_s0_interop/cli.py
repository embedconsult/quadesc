"""Command-line entry point for S0 interoperability evidence."""

from __future__ import annotations

import argparse
import importlib.metadata
import json
import sys
from datetime import UTC, datetime
from pathlib import Path

from .a2lcheck import parse_a2l
from .bundle import load_bundle_identity
from .capture import Capture, environment_metadata
from .client import profile_dict, run_external_s0, verify_captured_frames
from .profile import (
    CONTRACT_ARCHIVE_SHA256,
    CONTRACT_RELEASE,
    EXTERNAL_PROFILES,
    PYA2L_COMMIT,
    PYA2LDB_VERSION,
    PYXCP_COMMIT,
    PYXCP_VERSION,
    REFERENCE_PTY_SXI,
)
from .scenario import run_reference_suite


def _fixtures() -> Path:
    return Path(__file__).resolve().parents[2] / "fixtures"


def _dependency(name: str, expected_version: str, expected_commit: str) -> dict:
    dist = importlib.metadata.distribution(name)
    direct = dist.read_text("direct_url.json")
    direct_data = json.loads(direct) if direct else None
    actual_version = dist.version
    if actual_version != expected_version:
        raise RuntimeError(f"{name} version {actual_version} != {expected_version}")
    commit = ((direct_data or {}).get("vcs_info") or {}).get("commit_id")
    if commit != expected_commit:
        raise RuntimeError(f"{name} commit {commit!r} != {expected_commit}")
    return {"distribution": name, "version": actual_version, "commit": commit, "direct_url": direct_data}


def _metadata() -> dict:
    return {
        "generated_utc": datetime.now(UTC).isoformat(),
        "contract": {"release": CONTRACT_RELEASE, "archive_sha256": CONTRACT_ARCHIVE_SHA256},
        "dependencies": [
            _dependency("pyxcp", PYXCP_VERSION, PYXCP_COMMIT),
            _dependency("pya2ldb", PYA2LDB_VERSION, PYA2L_COMMIT),
        ],
        "environment": environment_metadata(),
    }


def _run(args: argparse.Namespace, capture: Capture) -> int:
    capture.write_json("environment.json", _metadata())
    fixtures = _fixtures()
    parsed = parse_a2l(fixtures / "led-s0.a2l", fixtures / "descriptors.json")
    a2l_result = parsed.to_result()
    bundle_identity = load_bundle_identity(fixtures / "bundle-metadata.json")
    capture.write_json("a2l-result.json", a2l_result)
    if args.command == "reference":
        result = run_reference_suite(capture, parsed, bundle_identity)
        result["profile"] = profile_dict("PTY allocated at runtime", REFERENCE_PTY_SXI)
    else:
        config = EXTERNAL_PROFILES[args.connection_profile]
        result = run_external_s0(args.port, capture, parsed, bundle_identity, config)
        result["endpoint"] = "external"
        result["transport"] = "sxi_uart"
        result["profile"] = profile_dict(args.port, config)
        result["warnings"] = [
            "The current 19200 core 0.1.2 firmware is diagnostic, not an XCP endpoint.",
            "Device open requires prior serial/modem/reset/BSL review and the serialized runner.",
        ]
        result["not_run"] = ["framing fault injection", "lost-response endpoint injection", "CAN"]
    result["a2l"] = a2l_result
    result["bundle_identity"] = bundle_identity.to_result()
    capture.write_json("result.json", result)
    verify_captured_frames(args.output / "raw-uart.jsonl")
    hashes = capture.finalize_manifest()
    print(json.dumps({"result": str(args.output / "result.json"), "status": result["status"], "sha256": hashes}, indent=2))
    return 0


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="Independent pyXCP S0 interoperability harness")
    subparsers = parser.add_subparsers(dest="command", required=True)
    reference = subparsers.add_parser("reference", help="run against the built-in PTY reference endpoint")
    reference.add_argument("--output", type=Path, required=True)
    endpoint = subparsers.add_parser("endpoint", help="run safe S0 checks against a reviewed external XCP serial path")
    endpoint.add_argument("--port", required=True)
    endpoint.add_argument("--connection-profile", required=True, choices=sorted(EXTERNAL_PROFILES))
    endpoint.add_argument("--output", type=Path, required=True)
    args = parser.parse_args(argv)
    capture: Capture | None = None
    try:
        capture = Capture(args.output)
        return _run(args, capture)
    except Exception as exc:
        if capture is not None:
            try:
                capture.write_json(
                    "failure.json", {"status": "fail", "error_type": type(exc).__name__, "error": str(exc)}
                )
                capture.finalize_manifest()
            except Exception as cleanup_exc:
                print(f"evidence cleanup failure: {type(cleanup_exc).__name__}: {cleanup_exc}", file=sys.stderr)
        print(f"interop failure: {type(exc).__name__}: {exc}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
