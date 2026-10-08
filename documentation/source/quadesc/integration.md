(fc-and-som-integration)=
# Flight-controller integration

QuadESC connects the SoM's control and sensing resources to four power stages. J9 joins the module to the mainboard, and J6 brings command and reporting signals to an external flight controller.

## Flight-controller boundary

J6 carries four DSHOT-named command inputs, telemetry, current reporting, GND and battery power. Match the [J6 pinout](connections.md#fc-interface-j6) to the external controller, paying particular attention to voltage levels and the `+BATT` pin.

Load an application that implements the command decoder and telemetry format expected by the FC. TI BSL does not operate the motors or generate these reports.

PB12 is the FC current-telemetry PWM route. It shares MCPWM1 with Motor 1, so both outputs use that timer's period. The output scale and the FC's interpretation need to agree for a useful current reading. See [shared control resources](control.md) for timer and enable relationships.

## SoM boundary

The carrier's buck output supplies the SoM's regulated input, and VAA supplies the analog reference. The module provides the MCU, pin multiplexing and communication resources. Its <a href="../som/power.html">power guide</a>, <a href="../som/interfaces.html">interface reference</a> and <a href="../som/boot.html">boot guide</a> cover these reusable functions.

The installed module connects through J9. For signal tracing, the alphanumeric pad names map SoM J1 to mainboard J9; use the [complete footprint table](connectors.md) and the underside viewing convention.

## Reset and failure behavior

Initialize `DRV_ENABLE` (PB10) and `INL_ENABLE` (PB11) low before configuring the drivers and starting PWM. Because both enable signals are shared, a reset or shutdown sequence can affect all four motors. Stopping PWM, disabling a driver and commanding zero torque have different effects on the power stage.

When connecting a control application, check the gate response during startup, MCU reset and loss of command input as well as during normal operation. SPI errors or stale ADC data can interrupt feedback, so include those conditions when checking the application's shutdown behavior.

## Trying bidirectional DShot

The [experimental DShot pinmux walkthrough](dshot-pinmux.md) describes a hardware approach for changing a signal pin between receive and transmit. On QuadESC, the flight controller sends the command and the ESC sends the reply. The walkthrough covers releasing the line, switching peripheral functions and rearming capture for the next command.
