# Getting started

The AM13E SoM ships with **TI's ROM bootloader (BSL) only**. Load your application before using its sensing, communications or timed outputs. The module can serve a custom carrier or a standalone development setup.

## Power and connections

1. Find J2 using the [connector locator](identity.md) and [pad table](connectors.md). Check pin 1 from the board view before making a cable.
2. Supply 3.3 V digital debug power at J2.1, or use regulated carrier power at the `+VDC` pads as described in [power](power.md). Check the probe's power direction before connecting it.
3. Choose a programming interface. For SWD, connect J2.2 SWDIO, J2.3 SWCLK, J2.7 reset and J2.5 ground, plus the probe's required supply/reference connection. For ROM UART, connect module TX at J2.4 to host RX and module RX at J2.6 to host TX, with ground at J2.5 and compatible logic levels.
4. To invoke BSL, hold J2.8 (`BSL_INVOKE`, PA6) high during reset or power-on. Use a host programmer that supports this MCU's TI BSL protocol. An ASCII serial terminal is not a BSL programmer.
5. Program and verify your application's image at its linked address. Release BOOT low and reset to run it. Follow [programming an application](firmware.md) for image-layout and debug checks.

Debug power runs the digital circuitry. ADC acquisition also needs VAA, and the CAN transceiver needs its 5 V supply. The [power diagram](power.md) shows these separate paths. [Boot and recovery](boot.md) gives the ROM CAN settings and recovery connections.

## Check your application's first output

Choose an observable startup action in your application, such as a debug breakpoint or UART message. For UART, configure UC4 on PA0/PA1 and use matching host settings. The [interface guide](interfaces.md) lists the mux values. If startup is silent, check the linked image address, boot selection and UART configuration using [debugging and peripheral checks](diagnostics.md).

## Explore the interfaces

Choose ADC, PWM, I²C, SPI or CAN resources using the [interface cheat sheet](interfaces.md). The [analog guide](analog.md) explains acquisition and conversion; [timed outputs](timing.md) covers shared timers, output pairs and capture.

For a custom carrier, start with [carrier design](carrier.md), which brings together power, shared pins, external devices and programming access.
