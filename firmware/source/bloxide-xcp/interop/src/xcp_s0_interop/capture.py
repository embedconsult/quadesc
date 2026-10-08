"""Raw and decoded evidence writers."""

from __future__ import annotations

import hashlib
import json
import os
import platform
import subprocess
import sys
import threading
import time
from pathlib import Path
from typing import Any


class Capture:
    def __init__(self, root: Path):
        self.root = root
        try:
            root.mkdir(parents=True, exist_ok=False)
        except FileExistsError as exc:
            raise FileExistsError(f"output directory must be fresh and must not already exist: {root}") from exc
        self._lock = threading.Lock()

    def record(self, file_name: str, **entry: Any) -> None:
        payload = {"monotonic_ns": time.monotonic_ns(), **entry}
        with self._lock, (self.root / file_name).open("a", encoding="utf-8", newline="\n") as stream:
            stream.write(json.dumps(payload, sort_keys=True, separators=(",", ":")) + "\n")

    def raw(self, direction: str, frame: bytes, *, status: str = "accepted", detail: str | None = None) -> None:
        self.record(
            "raw-uart.jsonl",
            transport="uart",
            direction=direction,
            raw_frame_hex=frame.hex(),
            octets=len(frame),
            status=status,
            detail=detail,
        )

    def sample(self, name: str, value: int, unit: str, address: int) -> None:
        self.record("decoded-samples.jsonl", symbol=name, value=value, unit=unit, address=address)

    def write_json(self, name: str, payload: Any) -> None:
        with (self.root / name).open("x", encoding="utf-8", newline="\n") as stream:
            stream.write(json.dumps(payload, indent=2, sort_keys=True) + "\n")

    def finalize_manifest(self) -> dict[str, str]:
        hashes: dict[str, str] = {}
        for path in sorted(self.root.iterdir()):
            if path.is_file() and path.name != "SHA256SUMS":
                hashes[path.name] = hashlib.sha256(path.read_bytes()).hexdigest()
        text = "".join(f"{digest}  {name}\n" for name, digest in hashes.items())
        with (self.root / "SHA256SUMS").open("x", encoding="ascii", newline="\n") as stream:
            stream.write(text)
        return hashes


def environment_metadata() -> dict[str, Any]:
    def output(*args: str) -> str:
        try:
            return subprocess.check_output(args, text=True, stderr=subprocess.STDOUT).strip()
        except (OSError, subprocess.CalledProcessError) as exc:
            return f"unavailable: {exc}"

    return {
        "python": sys.version,
        "python_implementation": platform.python_implementation(),
        "machine": platform.machine(),
        "platform": platform.platform(),
        "executable": sys.executable,
        "compiler": platform.python_compiler(),
        "uv": output("uv", "--version"),
        "cmake": output("cmake", "--version").splitlines()[0],
        "cc": output("cc", "--version").splitlines()[0],
        "uid": os.getuid(),
    }
