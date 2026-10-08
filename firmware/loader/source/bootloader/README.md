# AM13 CAN-FD bootloader ABI1

Standalone Rust reset loader, hardware-qualified2026-09-25 on the attached SOM.
Evidence and limits are in `notes/can-xcp-bootloader-r1/RESULT.md`. Current reservation32KiB; application origin0x8000, capacity222KiB,
end0x3f800; metadata occupies its own2KiB sector, ending at bank boundary0x40000.
Bank1 remains reserved so the application's existing calibration writes stay in
the opposite flash bank. CAL_A0x78000/CAL_B0x7c000 and their guards are preserved.

`contract.json` specifies transport, memory map, header, commit marker and command
sequence. CPU32MHz SYSOSC; explicit25MHz crystal startup, PLL400MHz/20/2 gives
CAN10MHz;500kbps nominal20tq/80%,2Mbps data5tq/80%, TDC offset4mtq.
MCAN_ICLK16MHz>=MCAN_FCLK10MHz, as required by TI SPRUJF2B27.4.2.
Clock startup is bounded; crystal failure stays in loader with CAN unavailable;
SWD recovery remains available. No oscillator autodetection.

Loader IDs0x710/0x711 CAN-FD+BRS64byte CTO; normal app calibration remains
classic500kbps IDs0x700/0x701. GET_ID0 identifies
`AM13E23019-CANBOOT/0.1.0/ABI1`. Reset with BOOT low, then CONNECT within3seconds;
invalid app stays indefinitely, any CONNECT holds until reset. BOOT high selects
TI ROM, not this loader. No app cooperation is required for reset+connect entry.

Standard XCP PROGRAM commands/opcodes and pyXCP0.29.18 framing were checked
against primary local source. Stop-and-wait only: use `last=True` for PROGRAM.
No DAQ, block transfer, sector info, seed/key, signature, encryption or rollback
protection. CRC detects corruption; it is not authentication. Firmware version
is descriptive. Header board/layout/ABI/length/CRC and vectors are checked.

Header staging must precede erase; metadata is invalidated before application
sectors. Sequential image bytes buffer into16-byte ECC-safe program granules.
PROGRAM count0 verifies full CRC and vectors. PROGRAM_RESET verifies again,
writes header then separate commit marker last, ACKs and resets. SRAM MSP must
be within conservative96KiB range and aligned; Thumb reset PC must lie in image.
RAM flash/read routines avoid instruction fetches from busy flash; GSC ownership,
sector bounds, dynamic protection and status checks guard each operation.
Non-erased/partial metadata reads handle ECC from RAM; invalid CRC cannot boot.
No mass erase, NONMAIN write, fuse/debug lock, static write protection or motor
operation is implemented. Logical bounds exclude loader, metadata and calibration
from raw app programming. Metadata access is a constrained staged transaction.

Application reinitializes its clocks/interrupts. Handoff stops CAN/SysTick, masks
interrupts, disables/clears NVIC, sets VTOR0x8000 and validated MSP/Thumb PC,
clears CONTROL/BASEPRI/FAULTMASK/MSPLIM. IRQs remain masked until app startup.

One-time install is SWD, using checked target identity, sector-only writes and
full loader readback. UART bootstrap was not tested. No factory NONMAIN change
is required by this source. Current SOM still has the earlier ROM bootPLL fix;
a physical untouched-factory NONMAIN test has not been performed. ROM CAN IDs3/4,
1Mbps and its25MHz direct-clock limitation are separate from this custom loader.

CLKDIV must be written after CLKEN; target readback admission checks this.
Normal CAN controller automatic error retry is enabled, with bounded polling.

Build: `bash notes/can-xcp-bootloader-r1/build-loader.sh` and `build-app.sh` on
bloxide1, Rust1.94.0 target thumbv8m.main-none-eabi. `package.sh` produces
bootloader.bin,app.bin,app.ab1. Host tests: test-core.sh and test-driver.sh.
Deployment/updater scripts are under notes/can-xcp-bootloader-r1/hardware;
bootstrap procedure and hardware evidence are packaged separately.
