#!/usr/bin/env python3
"""Run unmodified pinned pyXCP against the real Rust Session/SxI endpoint."""

from __future__ import annotations

import argparse
import errno
import json
import os
import pty
import select
import subprocess
import threading
import time
from collections import deque
from pathlib import Path

import serial
from pyxcp import types

from xcp_s0_interop.a2lcheck import parse_a2l
from xcp_s0_interop.bundle import load_bundle_identity
from xcp_s0_interop.capture import Capture, environment_metadata
from xcp_s0_interop.client import (
    ClientSession,
    assert_connect,
    assert_get_status,
    expect_error,
    read_scalar,
    run_external_s0,
    verify_bundle_identity,
)
from xcp_s0_interop.framing import FrameError, StreamFrameDecoder, decode_frame
from xcp_s0_interop.profile import EXTERNAL_19200_SXI


class EndpointDriver:
    def __init__(self, executable: Path, capture: Capture):
        self.capture = capture
        self.lock = threading.Lock()
        self.process = subprocess.Popen(
            [str(executable)],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            bufsize=1,
        )

    def request(self, command: str) -> list[str]:
        with self.lock:
            if self.process.poll() is not None:
                raise RuntimeError(f"endpoint exited {self.process.returncode}")
            assert self.process.stdin is not None and self.process.stdout is not None
            self.process.stdin.write(command + "\n")
            self.process.stdin.flush()
            lines: list[str] = []
            while True:
                line = self.process.stdout.readline()
                if not line:
                    raise RuntimeError("endpoint closed stdout before done")
                line = line.rstrip("\n")
                if line == "done":
                    break
                lines.append(line)
                self.capture.record("endpoint-events.jsonl", command=command, output=line)
            return lines

    def close(self) -> str:
        if self.process.poll() is None:
            self.request("quit")
        assert self.process.stderr is not None
        stderr = self.process.stderr.read()
        self.process.wait(timeout=2)
        return stderr


class PtyBridge:
    def __init__(self, driver: EndpointDriver, capture: Capture):
        self.driver = driver
        self.capture = capture
        self.master_fd, slave_fd = pty.openpty()
        self.path = Path(os.ttyname(slave_fd))
        os.close(slave_fd)
        self.stop = threading.Event()
        self.error: BaseException | None = None
        self.rx_sequence = 0
        self.rx_condition = threading.Condition()
        self.pending: deque[tuple[int, bytes, str]] = deque()
        self.request_decoder = StreamFrameDecoder()
        self.thread = threading.Thread(target=self._run, name="product-endpoint-bridge", daemon=True)
        self.thread.start()

    def _run(self) -> None:
        try:
            while not self.stop.is_set():
                if not select.select([self.master_fd], [], [], 0.01)[0]:
                    self._process_lines(self.driver.request(f"poll {time.monotonic_ns() // 1000}"))
                    continue
                try:
                    chunk = os.read(self.master_fd, 4096)
                except OSError as exc:
                    if exc.errno == errno.EIO:
                        time.sleep(0.001)
                        continue
                    raise
                if not chunk:
                    continue
                self.capture.raw("host_to_endpoint", chunk, detail="PTY read chunk")
                now_us = time.monotonic_ns() // 1000
                # Expire before feeding at the arrival timestamp, never silently
                # turn a late suffix into a fresh frame with observe_idle.
                self._process_lines(self.driver.request(f"poll {now_us}"))
                self._record_requests(chunk)
                lines = self.driver.request(f"feed {now_us} " + " ".join(f"{byte:02x}" for byte in chunk))
                self._process_lines(lines)
                with self.rx_condition:
                    self.rx_sequence += 1
                    self.rx_condition.notify_all()
        except BaseException as exc:  # retained and re-raised by close
            self.error = exc
            with self.rx_condition:
                self.rx_condition.notify_all()

    def _process_lines(self, lines: list[str]) -> None:
        for line in lines:
            if line.startswith("event Fenced("):
                # This is capture bookkeeping, not an upstream receive/parser
                # replacement. No rejected request may claim a later receipt.
                self.pending.clear()
                self.request_decoder.reset()
            elif line.startswith("tx "):
                self._publish(bytes.fromhex(line[3:]), consumed=True)
            elif line.startswith("dropped "):
                self._publish(bytes.fromhex(line[8:]), consumed=False)

    def _record_requests(self, chunk: bytes) -> None:
        try:
            frames = self.request_decoder.feed(chunk)
        except FrameError as exc:
            self.capture.record("attribution.jsonl", direction="host_to_endpoint", status="malformed", detail=str(exc))
            self.request_decoder.reset()
            return
        for raw in frames:
            counter, pdu = decode_frame(raw)
            self.pending.append((counter, pdu, raw.hex()))

    def _publish(self, frame: bytes, *, consumed: bool) -> None:
        status = "accepted" if consumed else "suppressed_for_recovery_test"
        self.capture.raw("endpoint_to_host", frame, status=status)
        response_counter, response_pdu = decode_frame(frame)
        request = self.pending.popleft() if self.pending else None
        self.capture.record(
            "attribution.jsonl",
            direction="exchange",
            consumed_by_client=consumed,
            request_counter=None if request is None else request[0],
            request_pdu_hex=None if request is None else request[1].hex(),
            request_frame_hex=None if request is None else request[2],
            response_counter=response_counter,
            response_pdu_hex=response_pdu.hex(),
            response_frame_hex=frame.hex(),
        )
        if consumed:
            os.write(self.master_fd, frame)

    def wait_for_rx(self, previous: int, timeout: float = 2.0) -> None:
        deadline = time.monotonic() + timeout
        with self.rx_condition:
            while self.rx_sequence == previous and self.error is None:
                remaining = deadline - time.monotonic()
                if remaining <= 0:
                    raise TimeoutError("endpoint bridge did not observe PTY input")
                self.rx_condition.wait(remaining)
        if self.error:
            raise self.error

    def close(self) -> None:
        self.stop.set()
        self.thread.join(2)
        if self.thread.is_alive():
            raise RuntimeError("PTY bridge did not stop")
        os.close(self.master_fd)
        if self.error:
            raise self.error


def raw_negative_matrix(bridge: PtyBridge, driver: EndpointDriver, capture: Capture) -> list[str]:
    checks: list[str] = []
    for name, raw in (
        ("len0", b"\x00"),
        ("len9", b"\x09"),
        ("checksum", bytes.fromhex("0200ff0000")),
    ):
        with serial.Serial(str(bridge.path), 19200, timeout=0.05, write_timeout=0.5) as port:
            previous = bridge.rx_sequence
            port.write(raw)
            port.flush()
            bridge.wait_for_rx(previous)
            assert not port.read(1), f"{name} unexpectedly produced a response"
        time.sleep(0.021)
        lines = driver.request(f"idle {time.monotonic_ns() // 1000}")
        assert any("Recovered" in line for line in lines), (name, lines)
        checks.append(f"{name}_discarded_without_response")

    partial_start = time.monotonic_ns()
    partial = bytes.fromhex("0255ff")
    with serial.Serial(str(bridge.path), 19200, timeout=0.05, write_timeout=0.5) as port:
        previous = bridge.rx_sequence
        port.write(partial)
        port.flush()
        bridge.wait_for_rx(previous)
        assert not port.read(1), "partial frame unexpectedly produced a response"
    time.sleep(0.021)
    now_us = time.monotonic_ns() // 1000
    driver.request(f"poll {now_us}")
    # Periodic bridge polling may already have reported the timeout. Require
    # the actual Truncated event after this partial input, not another timeout.
    events = [json.loads(line) for line in (capture.root / "endpoint-events.jsonl").read_text().splitlines()]
    assert any(event["monotonic_ns"] >= partial_start and "Truncated" in event["output"] for event in events)
    assert any("Recovered" in line for line in driver.request(f"idle {now_us}"))
    checks.append("truncation_expired_at_endpoint")

    assert any("Uart" in line for line in driver.request(f"uart-error {time.monotonic_ns() // 1000}"))
    time.sleep(0.021)
    assert any("Recovered" in line for line in driver.request(f"idle {time.monotonic_ns() // 1000}"))
    checks.append("injected_uart_error_discarded")
    capture.record("decoded-events.jsonl", behavior="negative_matrix", checks=checks)
    return checks


def run(endpoint: Path, output: Path) -> dict:
    capture = Capture(output)
    fixtures = Path(__file__).parents[1] / "fixtures"
    parsed = parse_a2l(fixtures / "led-s0.a2l", fixtures / "descriptors.json")
    bundle = load_bundle_identity(fixtures / "bundle-metadata.json")
    driver = EndpointDriver(endpoint, capture)
    bridge = PtyBridge(driver, capture)
    session: ClientSession | None = None
    checks: list[str] = []
    try:
        external = run_external_s0(str(bridge.path), capture, parsed, bundle, EXTERNAL_19200_SXI)
        checks.extend(external["checks"])

        session = ClientSession(str(bridge.path), capture, EXTERNAL_19200_SXI)
        assert_connect(session.master)
        assert_get_status(session.master)
        verify_bundle_identity(session.master, bundle)
        period = parsed.symbol("led.period_ms")
        session.master.setMta(period.address, period.address_extension)
        driver.request("drop-next-tx 0")
        try:
            session.master.download((321).to_bytes(2, "little"))
        except types.XcpTimeoutError:
            pass
        else:
            raise AssertionError("suppressed DOWNLOAD response did not time out")
        session.close()
        session = None
        loss_us = time.monotonic_ns() // 1000
        driver.request(f"loss {loss_us}")
        quiet_start = time.monotonic_ns()
        time.sleep(0.055)
        quiet_ms = (time.monotonic_ns() - quiet_start) / 1_000_000
        idle_lines = driver.request(f"idle {time.monotonic_ns() // 1000}")
        assert quiet_ms >= 50 and any("Recovered" in line for line in idle_lines)

        attribution_before = list(bridge.pending)
        session = ClientSession(str(bridge.path), capture, EXTERNAL_19200_SXI)
        assert_connect(session.master)
        assert_get_status(session.master)
        verify_bundle_identity(session.master, bundle)
        observed = read_scalar(session.master, period)
        if observed != 321:
            raise AssertionError(f"fresh readback expected applied 321, got {observed}")
        if attribution_before:
            raise AssertionError("suppressed request attribution was not consumed")
        checks.append("timeout_close_55ms_quiet_fresh_connect_readback_no_blind_retry")
        session.master.disconnect()
        session.close()
        session = None

        checks.extend(raw_negative_matrix(bridge, driver, capture))
        # A fresh client after all malformed cases proves the explicit idle
        # boundaries, rather than an in-band header scan, restored admission.
        session = ClientSession(str(bridge.path), capture, EXTERNAL_19200_SXI)
        assert_connect(session.master)
        assert_get_status(session.master)
        if read_scalar(session.master, period) != 321:
            raise AssertionError("state changed during malformed transport input")
        session.master.disconnect()
        checks.append("fresh_connect_after_malformed_idle_recovery")
        session.close()
        session = None

        # Standard unsupported command through the unchanged Master path.
        session = ClientSession(str(bridge.path), capture, EXTERNAL_19200_SXI)
        assert_connect(session.master)
        expect_error(lambda: session.master.transport.request(types.Command.GET_ID, 0), 0x20)
        session.master.disconnect()
        checks.append("unsupported_command_exact_error")
        session.close()
        session = None

        result = {
            "status": "pass",
            "endpoint": str(endpoint),
            "profile": "reviewed-19200-8n1",
            "pty_baud_is_physical_evidence": False,
            "quiet_ms": quiet_ms,
            "checks": checks,
            "client": {
                "pyxcp_version": "0.29.18",
                "pyxcp_commit": "016cf3e44364e9cd93966d144a39d342578a0391",
                "modified": False,
            },
            "claims": {
                "scoped_s0_pty_interoperability": True,
                "physical_uart": False,
                "native_arm64": False,
                "full_asam_conformity": False,
                "sum8_detects_all_corruption": False,
            },
        }
        capture.write_json("environment.json", environment_metadata())
        capture.write_json("result.json", result)
        return result
    finally:
        if session is not None:
            session.close()
        bridge.close()
        stderr = driver.close()
        if stderr:
            capture.write_json("endpoint-stderr.json", {"stderr": stderr})
        capture.finalize_manifest()


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--endpoint", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    print(json.dumps(run(args.endpoint.resolve(), args.output.resolve()), indent=2, sort_keys=True))


if __name__ == "__main__":
    main()
