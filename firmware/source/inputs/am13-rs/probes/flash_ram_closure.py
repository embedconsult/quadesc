#!/usr/bin/env python3
"""Conservative final-image placement preflight; never certifies RAM closure.

A final linked thumb ELF and manual closure attestation are still required. This
probe rejects obvious load/section violations and emits facts for that review.
It does not infer transitive callees, literals, IRQ/DMA accesses or stack fit.
"""
import argparse
import json
import re
import subprocess
import sys
from pathlib import Path

BANK0_END = 0x40000
SRAM_START, SRAM_END = 0x20000000, 0x20018000


def inside(start, size, low, high):
    return size >= 0 and low <= start <= high and start + size <= high


def covers(outer, size, inner, inner_size):
    return size >= 0 and inner_size > 0 and outer <= inner and inner + inner_size <= outer + size


def inspect(elf):
    raw = subprocess.run(["readelf", "-W", "-h", "-l", "-S", str(elf)], check=True, capture_output=True, text=True).stdout
    loads = []
    sections = []
    for line in raw.splitlines():
        parts = line.split()
        if parts and parts[0] == "LOAD" and len(parts) >= 7:
            # Type Offset VirtAddr PhysAddr FileSiz MemSiz Flags Align
            try:
                offset, vma, lma, filesz, memsz = (int(parts[n], 16) for n in (1, 2, 3, 4, 5))
            except ValueError:
                continue
            loads.append(dict(offset=offset, vma=vma, lma=lma, filesz=filesz, memsz=memsz))
        match = re.match(r"\s*\[\s*\d+\]\s+(\S+)\s+(\S+)\s+([0-9a-fA-F]+)\s+([0-9a-fA-F]+)\s+([0-9a-fA-F]+)\s+\S+\s+(\S+)", line)
        if match:
            name, section_type, address, offset, size, flags = match.groups()
            if "A" in flags:
                sections.append(dict(name=name, type=section_type, address=int(address, 16),
                                     offset=int(offset, 16), size=int(size, 16), flags=flags))
    errors = []
    if not re.search(r"Class:\s+ELF32", raw) or not re.search(r"Machine:\s+ARM", raw):
        errors.append("not an ARM ELF32 final image")
    if not loads:
        errors.append("no linked PT_LOAD segments")
    for i, seg in enumerate(loads):
        if seg["memsz"] < seg["filesz"] or seg["offset"] + seg["filesz"] > elf.stat().st_size:
            errors.append(f"PT_LOAD[{i}] malformed or truncated file extent")
        if seg["filesz"] and not inside(seg["lma"], seg["filesz"], 0, BANK0_END):
            errors.append(f"PT_LOAD[{i}] file-backed LMA outside bank0")
        if not (inside(seg["vma"], seg["memsz"], 0, BANK0_END) or inside(seg["vma"], seg["memsz"], SRAM_START, SRAM_END)):
            errors.append(f"PT_LOAD[{i}] VMA outside bank0/SRAM0-2")
    island = [s for s in sections if s["name"].startswith(".ram_flash")]
    if not any(s["size"] for s in island):
        errors.append("missing nonempty .ram_flash linked island")
    for s in sections:
        if not (inside(s["address"], s["size"], 0, BANK0_END) or inside(s["address"], s["size"], SRAM_START, SRAM_END)):
            errors.append(f"allocated section {s['name']} outside bank0/SRAM0-2")
        if not s["size"]:
            continue
        mapped = []
        for seg in loads:
            if not covers(seg["vma"], seg["memsz"], s["address"], s["size"]):
                continue
            if s["type"] != "NOBITS":
                delta = s["address"] - seg["vma"]
                if (not covers(seg["vma"], seg["filesz"], s["address"], s["size"])
                        or s["offset"] != seg["offset"] + delta
                        or not inside(seg["lma"] + delta, s["size"], 0, BANK0_END)):
                    continue
            mapped.append(seg)
        if len(mapped) != 1:
            errors.append(f"allocated section {s['name']} lacks one consistent PT_LOAD mapping")
    for s in island:
        if not inside(s["address"], s["size"], SRAM_START, SRAM_END):
            errors.append(f"{s['name']} not in SRAM0-2")
        if s["size"] and s["type"] == "NOBITS":
            errors.append(f"{s['name']} has no file-backed load bytes")
    return {"elf": str(elf), "loads": loads, "allocated_sections": sections, "errors": errors,
            "placement_preflight": "pass" if not errors else "reject",
            "ram_closure_proved": False,
            "unmet": ["transitive disassembly/relocations/literals/veneers", "all vectors and enabled IRQ/exception/DMA paths", "stack, time, watchdog, ECC and power-loss", "final exact linked image and manual attestation"]}


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("elf", type=Path)
    p.add_argument("--report", type=Path)
    args = p.parse_args()
    if not args.elf.is_file():
        p.error("final linked ELF missing")
    result = inspect(args.elf)
    data = json.dumps(result, indent=2) + "\n"
    if args.report:
        args.report.write_text(data)
    else:
        sys.stdout.write(data)
    return 0 if not result["errors"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
