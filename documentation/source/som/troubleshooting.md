# Troubleshooting and FAQ

Start with the active boot mode, power and wiring. A working SWD connection confirms digital access, but ADC and CAN also rely on separate supply connections.

## Why is the UART silent?

A newly received module has TI BSL only. Use a compatible BSL programmer to load an application; a serial terminal will not receive an application greeting. To invoke BSL, hold PA6 high during reset as described in [boot and recovery](boot.md).

For a loaded application, cross TX and RX, connect ground and match the baud rate and frame format to its code. Close other programs using the port. If it still does not start, inspect the reset handler and image layout over SWD.

## Why does CAN not respond?

Check the transceiver's 5 V rail, CANH/CANL wiring and termination. **PC0** controls the [termination switch](interfaces.md#can-termination-control). For ROM programming, use the CAN-FD settings and boot configuration in [boot and recovery](boot.md). For application traffic, match the configured bitrate and frame format to the other nodes.

## Why are ADC readings zero or near full scale?

Check the input connection and VAA/VREFHI first. A floating input, saturated divider or missing reference can make raw codes misleading. Follow the physical pad through its AIN identity to the selected ADC instance and channel. Compare input and reference voltages before applying the [conversion formula](analog.md#raw-counts-and-useful-units).

## Why does a timed output have the wrong period or stay driven after stop?

Check the timer's functional clock, prescaler, count mode and shared period. Stopping the counter does not necessarily release the pad. Configure the output actions and mux for the required idle state; see [timed outputs](timing.md).

## Is the whole side header a standard mikroBUS socket?

Each row has 12 pins, including CANH/CANL and additional signals and power connections. Use the [module pin table](connectors.md) to wire accessories.

(does-reset-reboot-the-module)=
## How do I reset the module?

Use `nRST` at J2.7 or a reset operation supported by your programmer. Reopening a terminal does not reset the MCU. PA6's level during reset selects whether you invoke BSL or start the programmed application.
