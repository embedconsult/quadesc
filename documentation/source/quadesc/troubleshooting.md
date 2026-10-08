# Troubleshooting and FAQ

Follow the signal from its source to the point where the result changes. The [connection guide](connections.md), [motor-control diagram](control.md) and [sensing formulas](sensing.md) help locate each part of that path.

## The board does not respond to flight-controller commands

The board ships with TI BSL only. Load a motor-control application with the FC's command decoder, then check J6 channel order, voltage levels and the configured protocol. [Flight-controller integration](integration.md) describes the signal paths; [first use](index.md#first-use) covers programming connections.

## A gate-driver register read fails

Check the selected chip-select line, driver supply and wake/enable state. All four drivers share the SPI signals, but each has its own chip select. Check the raw response before adjusting gain or other configuration. An internal MCU loopback check exercises the controller without sending a transaction to the external driver.

## Current looks plausible but has the wrong sign

The current-sense amplifier is inverting. Compare the sign with the definition and formula in [sensing](sensing.md), then check the reference and configured gain. A reversed sign can still produce plausible magnitudes while reversing the meaning of feedback. Looking at the raw code and the physical current path helps separate a sign error from a scale error.

## Phase voltage differs from the expected divider scale

The divider returns to the low-side source, so a ground-referenced voltage includes that node's contribution. Use the return-node correction in [sensing](sensing.md). Sampling time matters too: a sequential diagnostic scan reads the channels at different times rather than capturing a simultaneous waveform.

## Temperature rises while the raw count falls

That is the expected direction: lower counts mean a warmer board. The [nominal ADC-to-Celsius scale](sensing.md#board-temperature-from-the-thermistors) assumes a 10 kΩ thermistor at 25 °C and B = 3380 K; under those assumptions, 2048 counts is about 25 °C. Treat rail codes 0 and 4095 as invalid. Check the fitted sensor's curve, connection, VAA reference and selected channel before using the result for thermal control.

## A PWM command produces no measured waveform

Check the pad mux, peripheral power and clock, then trace the signal toward the driver. At the gate stage, check the common enables as well. Measuring at successive points helps distinguish a timer-output issue from an inactive driver.

## How much current can I run?

The [electrical targets](characterization.md) describe the intended design range. Usable current depends on cooling, ambient temperature and duration; the 40 A continuous and 70 A burst targets are not established operating ratings. Watch thermal behavior under the conditions of your assembled system.

## Which connector table should I use?

Use this manual's [PCB pad reference](connectors.md) for the mainboard and the <a href="../som/connectors.html">SoM pad reference</a> for module connectors. Check the connector name as well as its number because the boards have separate numbering.

For UART, CAN, the module power mux or boot recovery, use the <a href="../som/troubleshooting.html">SoM troubleshooting guide</a>.
