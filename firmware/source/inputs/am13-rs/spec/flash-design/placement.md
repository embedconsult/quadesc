# Record and linker reservation proposal

This is data-only layout advice for the later composition owner. Existing
`esc-som-board/examples/memory.x` remains512K FLASH/96K RAM; it currently has NO
calibration reservation and must not be treated as safe for persistence. SDK
GCC layout splits two256K banks and places `.TI.ramfunc` in SRAM3's code alias;
that SRAM3 configuration is not adopted. The selected platform leaves SRAM3 alone.

| Half-open physical address range | Proposed purpose |
|---|---|
| 0x00000..0x40000 | All vectors, code, constants, unwind tables, RAM init/load bytes and recovery image code; hard firmware bound256KiB |
| 0x40000..0x78000 | Bank1 unused; no executable/load section or live data fetch during flash work |
| 0x78000..0x78800 | Slot A, E=2048, global sector240, bank1 sector112 |
| 0x78800..0x7c000 | A protection-group guard,14336 bytes, never writable by service |
| 0x7c000..0x7c800 | Slot B, E=2048, global sector248, bank1 sector120 |
| 0x7c800..0x80000 | B protection-group guard,14336 bytes, never writable by service |
| 0x20000000..0x20018000 | Existing96KiB SRAM0–2 envelope; RAM island, vectors and stack must fit; SRAM3 unchanged |

Final32KiB reserved covers two16KiB groups under SDK interpretation and also
separates slots under the TRM8KiB interpretation. This is deliberate capacity
reservation, not a conclusion that protection mapping is resolved. Total record
storage4096bytes, guards28672bytes. No reserve in NONMAIN, DATA, FACTORY or ECC
aliases. Ordinary API accepts Slot A/B only and checked offset/length; it never
accepts an address from XCP/client/caller. Firmware upgrade/recovery procedures
must explicitly preserve these regions or declare reviewed reprovisioning and
account its wear. No cleanup erase of the previous record.

Future linker must define separate FLASH_CODE length0x40000, SRAM0–2, CAL_A,
CAL_B and GUARDS symbols, with no default output section in bank1. Reserve as
NOLOAD/address assertions, not FF-filled bytes that a binary writer might program.
Assert exact base/end, alignment, nonoverlap, RAM copy destination and stack
boundaries. Check every PT_LOAD's physical **file-backed** interval (including
RAM functions and .data LMAs), every allocated section VMA, vector target and
relocation/literal reference. Reject orphans, overlapping intervals, unchecked
flat binary padding, writable MAIN aliases, non-MAIN payload and bank swap.
Do not infer fit from `.text` size or ELF container bytes. The host probe uses
retained real T05 ELF program headers and separately records total SRAM memory
extents; it does not prove a nonexistent integrated T13/T17 image fits.

Current diagnostic has48208 bytes, rounded historical readback49152 bytes. If
loaded at0, this leaves213936 bytes below bank0 end, but those lengths alone do
not validate sparse load segments or unseen MAIN. Retained T05 independent linked
fixture is8440 contiguous load bytes, leaving253704 bytes. Exact retained ELF
and source provenance are in `image-fit.json`; changed path-build debug bytes are
not relabeled identical. Full512KiB readable backup and sector guards remain a
future T04 requirement; a49KiB backup is incomplete for this reservation.

## Portable geometry mapping

| Candidate | E | G | M=roundup(640,G) | marker | Tail | body/hardware programs | normal portable commands |
|---|---:|---:|---:|---|---:|---|---:|
| Preferred conditional word-line isolation |2048|256|768|[768,1024)|1024|3 portable body +1 marker;64 total16B hardware programs|38|
| Conditional word isolation |2048|16|640|[640,656)|1392|40 body +1 marker;41 total16B programs|75|

For G256 body alignment padding[640,768) stays logical/physical FF as required;
programming a complete256-byte command includes these FF bytes once with generated
ECC. Marker is all-zero256 bytes in a separate word line. A torn subcommand is
contained within the one outstanding portable granule only IF verified physical
isolation holds. Already finished portable granules must remain stable. Hardware
word-line meaning does not by itself establish that predicate. All normal programs
start after full2048-byte healthy erased verification; program each word once,
even when data is FF; never repair or resume a partial granule after reset/error.

If qualification instead supports G16, body+marker share a word line but separate
16B words/ECC; that is legal only with demonstrated/authoritative interword
isolation and word-line operation limit. Do not switch G across boots: geometry
and codec are provisioning-lifetime identities. Different G changes marker offset
and requires separately reviewed migration/reprovisioning. E1024 compatibility
arithmetic in the probe is a negative/alternative reference control, not selected
AM13 geometry. G>256 is unsupported, not permission to split disturbance domains.
