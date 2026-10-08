# Boot and recovery

The SoM ships with **TI ROM BSL only**. Use a compatible BSL host programmer or SWD probe to load your application. ROM BSL and any application UART monitor use different protocols.

## Boot and debug connections

J2 brings the development signals to an eight-pin row:

| Pin | Function |
|---|---|
| J2.1 | 3.3 V digital debug power input |
| J2.2 / J2.3 | SWDIO / SWCLK |
| J2.4 / J2.6 | UART TX / RX, from the module's perspective; PA0 / PA1 |
| J2.5 | Ground |
| J2.7 | `nRST` |
| J2.8 | `BSL_INVOKE` on PA6 |

See [power](power.md) before connecting a debugger supply and [connector orientation](identity.md) before making a cable. For UART, cross TX/RX and use compatible logic levels with a common ground.

PA6 selects ROM boot at reset. R17 pulls it to ground through 10 kΩ. Hold BOOT high at reset or power-on to invoke BSL; release it low and reset to start a programmed application. Carrier circuitry connected to PA6 must allow the required level during reset.

## Image layout and recovery

Program at the address required by your application's linker layout. Check the destination and size so the write stays clear of reserved memory and device configuration. Verify the programmed data, release BOOT low, then reset the MCU. See [programming an application](firmware.md).

SWD gives access to the core even when the application vectors are erased. Connect SWDIO, SWCLK, reset, ground and the probe's required power/reference connection. A reset pulse followed by prompt attachment can help with a blank device. Attachment timing depends on the probe and setup.

## ROM CAN

The ROM CAN configuration described here uses **1 Mbit/s nominal and data, CAN-FD without bit-rate switching, and request/response IDs 3/4**. Hold **BOOT high at reset or power-on** and supply the CAN transceiver's 5 V rail as well as digital power. These are bootloader settings; your application configures its own CAN operation.

This configuration uses the **SYSOSC-sourced 80 MHz boot PLL**. A device may need its boot configuration set before ROM CAN responds. Boot-configuration changes include updating the BCR checksum while preserving unrelated configuration and security bytes. Use the device-specific programming procedure for those changes.

## If programming or startup fails

| Symptom | Next check |
|---|---|
| SWD does not attach | Digital power, ground, SWD wiring, reset and probe attachment timing |
| UART programmer gets no BSL response | PA6 high during reset, crossed TX/RX and host support for this MCU's BSL protocol |
| ROM CAN does not respond | Transceiver power, wiring, termination, CAN-FD settings and boot configuration |
| Programming verifies but the application is silent | BOOT low at reset, image destination, reset vector and application's clock/UART setup |
| Application repeatedly restarts | Supply stability, reset line and reset cause available through the device registers |
