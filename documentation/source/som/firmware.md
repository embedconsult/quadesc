(firmware-and-examples)=
# Programming an application

Build an image for the AM13E23019, load it through TI BSL or SWD, and reset to start it. Factory ROM provides TI BSL. Choose an application and toolchain suited to your connected devices.

## What programming and debugging mean

**Programming** writes an application into flash so it remains after power is removed. **Debugging** connects a host computer to the MCU so you can stop execution, inspect registers and memory, set breakpoints and resume it. Connecting a debugger alone does not install an application.

| Action | What happens |
|---|---|
| Connect / halt | The debugger gains access to the core and stops execution. |
| Load Program | The tool writes executable code and loads its symbols; flash programming needs device-specific support. |
| Load Symbols | The debugger learns function and variable names without writing the application. |
| Breakpoint | Execution stops when it reaches the selected instruction. |
| Step / resume | Execute one instruction or source line, or continue until the next stop. |
| Reset | Restart through the device's boot path; this differs from resuming a halted application. |

Start with a simple check: connect, halt, inspect the program counter, then resume. Next, use a breakpoint at your application's entry point to establish that startup reaches it. Source-line stepping requires an executable built with debug information; otherwise use symbols, registers and disassembly.

## Use the firmware supplied in this repository

The module's factory ROM and the repository's application are separate. The repository also contains a custom reset loader. Installing only an application that expects this loader does not make a blank module boot it.

Follow the [repository firmware instructions](https://github.com/embedconsult/quadesc/blob/main/firmware/README.md) to identify the delivered images, their addresses, loader requirements and current installation limitations. Then use [XDS110 setup](xds110.md) for the physical connection and CCS session. Do not move an image to address zero merely to make it start.

(choose-a-starting-point)=
## Build for the MCU

For Rust, the MCU target is `thumbv8m.main-none-eabi`. Installing that target makes the compiler's target libraries available:

```sh
rustup target add thumbv8m.main-none-eabi
```

A working executable also needs device startup code, an interrupt vector table, a linker memory layout and peripheral initialization. The target triple alone does not provide these. For another language or toolchain, select the AM13E23019 device and its matching startup and linker files. Build host programming utilities for the computer running them, separately from the MCU image.

## Load and start the image

For TI's XDS110, use the [probe wiring, power selection and CCS programming procedure](xds110.md).

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
