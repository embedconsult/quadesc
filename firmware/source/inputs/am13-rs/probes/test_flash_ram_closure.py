#!/usr/bin/env python3
"""Small linked ARM placement control for flash_ram_closure.inspect."""
import subprocess
import tempfile
from pathlib import Path

from flash_ram_closure import inspect


def run(*args):
    subprocess.run(args, check=True, capture_output=True, text=True)


def main():
    with tempfile.TemporaryDirectory(prefix="t16-ram-placement-") as directory:
        root = Path(directory)
        (root / "island.s").write_text('''
            .syntax unified
            .thumb
            .section .vector_table,"a",%progbits
            .word 0x20001000
            .word island + 1
            .section .ram_flash,"ax",%progbits
            .global island
        island:
            nop
            bx lr
            .section .bss,"aw",%nobits
            .space 4
        ''')
        (root / "layout.ld").write_text('''
            PHDRS { rom PT_LOAD FLAGS(4); ram PT_LOAD FLAGS(7); }
            SECTIONS {
                . = 0;
                .vector_table : { *(.vector_table) } :rom
                . = 0x20000000;
                .ram_flash : AT(0x1000) { *(.ram_flash) } :ram
                .bss (NOLOAD) : { *(.bss) } :ram
            }
        ''')
        run("arm-none-eabi-as", "-mcpu=cortex-m33", "-mthumb", "-o", root / "island.o", root / "island.s")
        run("arm-none-eabi-ld", "-T", root / "layout.ld", "-o", root / "valid.elf", root / "island.o")
        valid = inspect(root / "valid.elf")
        assert valid["placement_preflight"] == "pass", valid["errors"]
        assert not valid["ram_closure_proved"]
        # Corrupt only the SRAM PT_LOAD physical address: VMA/file coverage
        # stays intact while its file-backed load bytes move into bank1.
        blob = bytearray((root / "valid.elf").read_bytes())
        program_headers = int.from_bytes(blob[28:32], "little")
        entry_size = int.from_bytes(blob[42:44], "little")
        count = int.from_bytes(blob[44:46], "little")
        for index in range(count):
            entry = program_headers + index * entry_size
            if int.from_bytes(blob[entry + 8:entry + 12], "little") == 0x20000000:
                blob[entry + 12:entry + 16] = (0x40000).to_bytes(4, "little")
                break
        else:
            raise AssertionError("missing SRAM PT_LOAD control")
        (root / "bank1.elf").write_bytes(blob)
        rejected = inspect(root / "bank1.elf")
        assert rejected["placement_preflight"] == "reject", rejected
        # A truncated file extent must not cover the still allocated island.
        truncated = bytearray((root / "valid.elf").read_bytes())
        truncated[entry + 16:entry + 20] = (1).to_bytes(4, "little")
        (root / "truncated.elf").write_bytes(truncated)
        rejected = inspect(root / "truncated.elf")
        assert rejected["placement_preflight"] == "reject", rejected
        # An allocated unrelated orphan is also a placement failure.
        (root / "orphan.bin").write_bytes(b"\0\0\0\0")
        run("arm-none-eabi-objcopy", "--add-section", f".orphan={root / 'orphan.bin'}",
            "--set-section-flags", ".orphan=alloc,load,readonly,data",
            "--change-section-vma", ".orphan=0x20001000",
            root / "valid.elf", root / "orphan.elf")
        rejected = inspect(root / "orphan.elf")
        assert rejected["placement_preflight"] == "reject", rejected
        assert any(".orphan" in error for error in rejected["errors"]), rejected
        print("valid mapped ARM island: pass; bank1 LMA, truncated load and orphan: reject; RAM closure: unproved")


if __name__ == "__main__":
    main()
