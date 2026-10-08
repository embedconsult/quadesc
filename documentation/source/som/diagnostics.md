(diagnostic-command-cheat-sheet)=
# Debugging and peripheral checks

Use SWD to inspect the MCU and electrical measurements to check the interface at its connector. UART messages, ADC reads and loopback tests require code that implements them; TI BSL provides programming access, not an application command shell.

(host-commands)=
(uart-protocol)=
## Establish core and UART access

Connect SWDIO/SWCLK at J2.2/J2.3, reset at J2.7 and ground at J2.5, with the probe's required power/reference connection. If attachment fails, check digital power and reset, then try attaching promptly after a reset pulse.

For an application UART, cross module TX J2.4 to host RX and module RX J2.6 to host TX. Configure UC4 on PA0/PA1, mux 7. Match the application's baud rate, data bits, parity and stop bits, and close other programs using the serial port. A ROM BSL session needs a BSL-compatible host tool instead.

(collecting-a-report)=
## Check one peripheral at a time

| Interface | Setup to check | Expected observation |
|---|---|---|
| ADC | VAA/VREFHI, analog pad mode, ADC channel and acquisition duration | Raw counts follow the applied voltage; nominally `raw × Vref / 4096` volts |
| PWM | Peripheral clock, pad mux, counter mode, period and output actions | Period and duty match timer ticks; start/stop levels match the configured output path |
| SPI | UC3 on PC6/PC4/PC2, selected chip select, peer mode and word length | Clock and chip select frame a transaction at the external device |
| I²C | UC2 SDA PA22, SCL PA23, pull-ups and peer address | Released lines rise and the addressed device acknowledges |
| CAN | Transceiver 5 V, CANH/CANL, termination and bit timing | The controller communicates using the same frame format and bitrate as its peers |

An internal SPI loopback can isolate a controller issue, but it does not exercise the connector or external device. For an external transaction, inspect the signals at the peer and its response.

## Separate acquisition from conversion errors

Keep raw ADC counts available when checking a sensor. Compare the input and reference voltages, then apply the carrier's gain, offset or divider scale. A sequential sweep visits different inputs at different times; use a shared trigger across ADC cores when simultaneous measurements are required. See [analog acquisition](analog.md).

## Check timing at the wire

Measure the PWM period, extreme duties, first pulse after start and level after stop. A stopped timer can leave an output driven. For capture, compare stored timestamps with the input waveform, account for timer wrap and detect overwritten or missing events. A software completion flag can precede the end of an output waveform.

A crystal measurement relative to SYSOSC compares two clocks; it does not establish absolute crystal accuracy. Derive each peripheral's timing from its selected functional clock, as described in [timed outputs and clocking](timing.md).
