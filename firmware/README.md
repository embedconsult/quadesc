# Using the supplied firmware

This directory contains the board application and its supporting libraries in `source/`, existing application artifacts in `application/`, and a separate loader with its own source tree in `loader/`.

The application is motor-carrier firmware for BeagleConnect QuadESC, running on BeagleConnect Torque. It is not a generic demonstration for every carrier. Keep motor power disconnected for first programming and debug checks. The source describes inactive PWM and driver-enable outputs at startup, but that does not replace a check of the connected hardware.

## Which file do I use?

Paths below are relative to the repository root. Use files from the same checkout together.

| File | Purpose | Destination / use |
|---|---|---|
| `firmware/loader/bootloader.bin` | Custom reset loader, distinct from TI ROM BSL | Raw flash image at `0x00000000`; reserves the first 32 KiB |
| `firmware/application/firmware.bin` | Application code and initial data | Raw flash image at `0x00008000`; never program it at zero |
| `firmware/application/firmware.ab1` | Application update package with a 32-byte header followed by the application | Input to a compatible custom-loader updater; not a raw image to flash at `0x8000` |
| `firmware/application/firmware.elf` | Application executable and symbols | Load symbols for debugging; review loader behavior before using it to program flash |
| `firmware/loader/bootloader.elf` | Reset-loader executable and symbols | Debugging the custom loader |
| `firmware/application/firmware.a2l`, `XCP_104.aml` | XCP measurement/calibration descriptions | Host tooling metadata, not MCU flash images |
| `firmware/application/contract.json`, `board-profile.json` | Application parameter and board definitions | Check the supported controls and hardware mapping |

The shipped application ELF has a `PT_LOAD` segment at address zero containing ELF headers, as well as its application sections starting at `0x8000`. A loader that programs every segment could overwrite the reset loader. For these supplied artifacts, use the raw BIN with an explicit address and reserve ELF for **Load Symbols**, unless the programmer's section/segment handling has been verified. Inspect with:

```sh
readelf -l firmware/application/firmware.elf
readelf -S firmware/application/firmware.elf
sha256sum firmware/application/firmware.bin firmware/loader/bootloader.bin
```

Compare hashes with `firmware/build-report.json`. Its build and test evidence applies to the recorded artifacts; it is not evidence that this XDS110 procedure has been tested.

## First installation and normal startup

1. Follow [XDS110 wiring and CCS setup](../documentation/source/som/xds110.md). Connect and halt the core before changing memory. Confirm the exact AM13E23019 device profile and flash support are installed.
2. For a first custom-loader installation, program and verify `bootloader.bin` as a raw binary at **`0x00000000`**, using an application-independent raw-binary flash operation from the device-supported programmer. CCS menu availability depends on the installed device package; do not treat a memory-view RAM write as verified flash programming. Erase only the required loader sectors, preserving configuration and calibration.
3. Install `firmware.ab1` through a compatible CAN updater implementing `firmware/loader/contract.json`. The custom loader uses **500 kbit/s nominal, 2 Mbit/s data, CAN-FD with bit-rate switching**, standard request/response IDs **`0x710` / `0x711`**. Supply the CAN transceiver's 5 V rail, common ground and suitable bus termination. Reset with PA6/BOOT low and send CONNECT within three seconds. A successful CONNECT keeps the loader resident; an invalid application also keeps it resident.
4. Require image verification and the updater's final commit/reset operation. The loader validates the image and committed metadata before it will jump to `0x8000`. After reset, allow the three-second connection window to expire without connecting an updater.

**Current repository limitation:** a ready-to-run CAN updater and the `tools/firmware/build.sh` script referenced by imported source READMEs are not included in this checkout. Step 3 therefore describes the required protocol, not an available copy-and-paste command. Obtain the matching updater before attempting a complete first installation. See the [loader contract](loader/contract.json) for the exact sequence. This documentation does not claim a completed XDS110 installation test.

Writing `firmware.bin` at `0x8000` by SWD writes application bytes only. It does **not** create the valid header and commit marker in `0x3f800..0x3ffff`, and may invalidate metadata for an older image. Installing both BIN files alone is therefore insufficient for normal startup. Keep application writes below `0x3f800`; preserve reserved bank 1, calibration at `0x78000` and `0x7c000`, and NONMAIN configuration/security data. Do not use a mass erase to work around a missing updater.

PA6 high selects **TI ROM BSL**, not this custom loader. ROM CAN settings in the hardware manual and custom-loader CAN settings above are different protocols and bit timings.

## A concrete debug session

For an application already installed with valid loader metadata:

1. Connect XDS110 in SWD mode using the [probe guide](../documentation/source/som/xds110.md). Disable automatic program download when launching the session.
2. Halt the Cortex-M33 core. Use **Load Symbols** with `firmware/application/firmware.elf` from the exact installed build. This adds names without rewriting flash.
3. Set a hardware breakpoint at `Reset` in the application symbols. Its address should be in the application region starting at `0x8000`; use disassembly if source lines are unavailable. The delivered build has debug information disabled (`debug = 0`), so source-level stepping is limited.
4. Reset the MCU, then resume it so the custom loader can run. Leave PA6 low and do not send a loader CONNECT. After the loader window, the core should stop at the application's reset entry. If it stays in the loader, check image metadata and CRC before investigating application peripherals.
5. Step through startup in disassembly, or resume and halt again to inspect the program counter and fault state. A successful first check is reaching the application and remaining responsive to halt/resume; it does not qualify motor operation.
6. End debugging, reset and confirm startup again without debugger assistance, retaining external power if probe power was removed.

The supplied application uses CAN XCP for measurement/control at **500 kbit/s classic CAN**, request/response IDs **`0x700` / `0x701`**. This differs from the custom loader. Its UART diagnostic basis is **19,200 baud, 8 data bits, no parity, one stop bit**, with UC4 TX/RX on J2.4/J2.6. Cross TX/RX to a 3.3 V logic-level serial adapter and use J2.5 ground. An open terminal by itself is not a programming tool, and the ROM does not supply the application's diagnostic commands. Use the application's [diagnostic specification](source/drone-esc-firmware/spec/am13-diagnostic.md) for supported messages. Do not infer that a missing UART banner means the flash write failed.

## Source and alternative programming tools

The application and loader source trees are independent. Their imported READMEs describe the original build environment; some referenced scripts and host tools are absent here. Use the supplied artifacts for the workflow above, or restore the matching build tooling before promising a reproducible source build. `source-provenance.json`, `toolchain-versions.json` and `build-report.json` record the delivered inputs and results.

`drone-flasher-swd/flash-swd.sh` is a separate **Jetson GPIO/OpenOCD** programming path. It does not drive XDS110; do not run its Jetson pinmux commands on a desktop to configure this probe.

See the [BeagleConnect Torque programming guide](../documentation/source/som/firmware.md) for terminology and general image requirements, [boot and recovery](../documentation/source/som/boot.md) for TI ROM access, and [BeagleConnect QuadESC manual](../documentation/source/quadesc/index.md) for carrier connections. Documentation CI builds the hardware manuals independently of these firmware trees.
