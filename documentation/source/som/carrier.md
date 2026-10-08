# Designing a reusable carrier

A carrier turns the SoM into the controller for your project. It supplies power, connects external devices and conditions signals for the MCU. The module's underside array provides broad access to the MCU, while the side headers are convenient for connecting a smaller set of interfaces.

## Plan the connections

Start with the [connector table](connectors.md) to locate each signal, then use the [interface cheat sheet](interfaces.md) to choose its peripheral function. A physical pad can serve different alternate functions, so check for shared pins before laying out the carrier.

Supply regulated power to `+VDC` and provide VAA for analog acquisition. The digital debug supply alone does not power every interface. The [power chapter](power.md) explains the load switch, regulator and power mux.

For UART, connect module TX to the external receiver and module RX to the external transmitter, with compatible electrical levels and a common ground reference. UC4 also serves ROM access, so connected devices should release the lines when you use the bootloader.

I²C shares SDA and SCL among devices. Include the on-module pull-ups when choosing any additional pull-ups, and check device addresses and total bus loading. For SPI, match mode, word length and chip-select timing to the connected device; configure those settings explicitly in your application.

## CAN integration

Connect CANH and CANL at J5 or the expansion row. Termination belongs at the ends of the bus; select the module's termination to suit its position in the network. **PC0, MCU pin 97**, drives the termination switch through `CAN_TERM` and is available at **J1.B9**. See [CAN termination control](interfaces.md#can-termination-control).

The module includes the physical CAN transceiver. Its 5 V supply and the MCU's digital supply both need power for bus communication. Bitrate and message handling come from the application.

## Mechanics and production

J1 is an underside solder array with a **1.27 mm grid** and alphanumeric pad names. Use the [mechanical reference](identity.md) to orient the module and plan its mating footprint. Leave access to reset, boot invoke and debug so you can program and troubleshoot the assembled carrier.

The two side headers each have 12 pins. They provide another way to connect a carrier; use their full pin tables when choosing mating connectors and routing power.

## First operation

Begin with the power rails and reference, then try one external device at a time. Check signal levels when either board is unpowered to avoid feeding an input through its protection structures. For analog inputs, account for divider range, source impedance and settling. For timed outputs, check the idle level and waveform at the actual connector before attaching the load.
