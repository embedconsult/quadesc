(firmware-and-examples)=
# Programming an application

Build an image for the AM13E23019, load it through TI BSL or SWD, and reset to start it. Factory ROM provides TI BSL. Choose an application and toolchain suited to your connected devices.

(choose-a-starting-point)=
## Build for the MCU

For Rust, the MCU target is `thumbv8m.main-none-eabi`. Installing that target makes the compiler's target libraries available:

```sh
rustup target add thumbv8m.main-none-eabi
```

A working executable also needs device startup code, an interrupt vector table, a linker memory layout and peripheral initialization. The target triple alone does not provide these. For another language or toolchain, select the AM13E23019 device and its matching startup and linker files. Build host programming utilities for the computer running them, separately from the MCU image.

## Load and start the image

1. Select ROM BSL or SWD using the connections in [boot and recovery](boot.md).
2. Use the image's linker layout to determine its programming address and size. Keep writes within application memory and clear of reserved regions and device configuration. A raw binary does not carry its own destination address.
3. Program the image and read it back or use the programmer's verification operation.
4. Release PA6/BOOT low and reset for normal startup. Use SWD to check the reset handler if the application does not reach its first observable action.

An image linked for address zero cannot simply be placed in a relocated application slot. Relocation requires matching startup, vector and boot arrangements. Preserve unrelated configuration and security bytes when making device-specific boot-configuration changes.

## Initialize the interfaces you use

Configure peripheral power and clock sources before calculating timings. Set each pad's mux to the selected peripheral, then configure its protocol or waveform. The [pin and mux table](interfaces.md) gives the module's communication routes. For GPIO, select the intended output level before connecting an output to a load.

ADC setup also needs VAA/VREFHI, analog pad configuration, channel selection and acquisition timing. Use [analog acquisition](analog.md) for the distinction between physical AIN numbers and converter channels. PWM outputs share module periods and pair resources as described in [timed outputs](timing.md).

## Debug startup

SWD provides core access independently of an application UART. Inspect the reset handler, clock initialization and exception state when an image does not start. With UART logging, configure UC4 TX on PA0 and RX on PA1, mux 7, then match the host baud rate and frame format to your code. BSL and application UART traffic use different protocols even though they can share those pins.

See [debugging and peripheral checks](diagnostics.md) for checks at the connector and peripheral, and [boot and recovery](boot.md) for a blank or unresponsive device.
