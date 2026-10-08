#!/usr/bin/env python3
"""Normative 30/80ms late-suffix regression through the actual product bridge."""
from __future__ import annotations
import argparse
import json
import time
from pathlib import Path
import serial
from product_endpoint_pty import EndpointDriver, PtyBridge
from xcp_s0_interop.capture import Capture


def verify(endpoint: Path, output: Path) -> list[dict]:
    results = []
    for delay_ms in (30, 80):
        capture = Capture(output / f"late-suffix-{delay_ms}ms")
        driver = EndpointDriver(endpoint, capture)
        bridge = PtyBridge(driver, capture)
        try:
            with serial.Serial(str(bridge.path), 19200, timeout=0.1) as port:
                def send(raw: bytes) -> None:
                    previous = bridge.rx_sequence
                    port.write(raw)
                    port.flush()
                    bridge.wait_for_rx(previous)

                send(bytes.fromhex("0200"))
                time.sleep(delay_ms / 1000)
                send(bytes.fromhex("ff0001"))
                assert port.read(11) == b"", "expired CONNECT suffix emitted a response"
                # Even an intact next frame after quiet cannot implicitly recover.
                send(bytes.fromhex("0200ff0001"))
                assert port.read(11) == b"", "discard mode scanned an in-band header"
                status = driver.request(f"status {time.monotonic_ns() // 1000}")
                assert any("state=Recovery" in line for line in status), status
                events = [json.loads(line) for line in (capture.root / "endpoint-events.jsonl").read_text().splitlines()]
                assert not any(e["output"].startswith("provider ") for e in events), "late suffix reached backend"
                assert any("Truncated" in e["output"] for e in events), "missing actual expiry"
                feeds = list(dict.fromkeys(e["command"] for e in events if e["command"].startswith("feed ")))
                gap_us = int(feeds[1].split()[1]) - int(feeds[0].split()[1])
                assert gap_us >= 20_000
                time.sleep(0.055)
                assert any("Recovered" in line for line in driver.request(f"idle {time.monotonic_ns() // 1000}"))
                send(bytes.fromhex("0200ff0001"))
                recovered = port.read(11)
                assert recovered == bytes.fromhex("0800ff010008080001011a"), recovered.hex()
                result = {"scheduled_gap_ms": delay_ms, "observed_gap_us": gap_us,
                          "late_suffix_response": "", "backend_admissions_before_idle": 0,
                          "in_band_recovery": False, "fresh_connect_response": recovered.hex(), "status": "pass"}
                capture.write_json("result.json", result)
                results.append(result)
        finally:
            bridge.close()
            driver.close()
            capture.finalize_manifest()
    return results


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--endpoint", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    print(json.dumps(verify(args.endpoint.resolve(), args.output.resolve()), indent=2))
