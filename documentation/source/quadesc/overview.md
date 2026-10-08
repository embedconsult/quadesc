# Board overview and motor channels

QuadESC includes four three-phase power stages and an installed AM13E SoM. Each channel has a gate driver, three half bridges, low-side current sensing and divided phase-voltage sensing. The AM13E SoM at J9 supplies the controller and connects these signals to its timers, ADCs and communication peripherals.

The channels are **Motor 1 through Motor 4**. Their phase connections are J2 through J5 respectively. U1–U4 are **DRV8323SRTAR** gate drivers; U5–U16 are **CSD88599Q5DC** half-bridge devices.

## Board views

```{image} ../shared/assets/mainboard-board-top.svg
:alt: QuadESC mainboard top assembly and connector locator
:target: _images/mainboard-board-top.svg
```

```{image} ../shared/assets/mainboard-board-bottom.svg
:alt: QuadESC mainboard underside assembly and connector locator
:target: _images/mainboard-board-bottom.svg
```

Use these assembly views to find connectors and components. Reference labels match the KiCad board files. Select an image for a closer look.

**Top** looks down at F.Cu. **Bottom** looks directly at B.Cu after turning the board left-to-right, keeping the same top edge up. Left and right therefore reverse; the bottom drawing is not a view through the board. Use the [connector pad table](connectors.md) for electrical assignments.

The underside carries J9, the SoM mating footprint, and J1, the two battery terminals. Motor solder terminals are J2 (Motor 1), J3 (Motor 2), J4 (Motor 3) and J5 (Motor 4); each has three electrical pads.

## Major components

| Resource | Component | Purpose |
|---|---|---|
| U1–U4 | DRV8323SRTAR | Gate drive, current-sense amplification and status/register interface |
| U5–U16 | CSD88599Q5DC | Twelve half bridges across four channels |
| U17 | TPS54360BDDAR | Battery-to-carrier buck regulator |
| U18 | TPS7A4701RGWR | Analog supply/reference regulator |
| J1 | Two battery solder terminals | 1: +BATT; 2: GND |
| J2–J5 | Three solder terminals per motor | 1: phase A; 2: phase B; 3: phase C |
| J6 | Eight-pin FC interface | Ground, battery, current, telemetry and four command inputs |
| J7 | ARM/JTAG header | Debug access through the carrier |
| J8 | Two-pin buzzer interface | Buzzer signal and ground |
| J9 | SoM receptacle | 100-pad LGA plus additional physical pads |

Component numbers belong to each board's own schematic. For example, mainboard U2 is a gate driver and SoM U2 is the CAN transceiver. The connector and circuit references in this manual refer to the mainboard unless they explicitly name the SoM.

## What the circuitry enables

PWM from the SoM commands the gate drivers, which switch the half bridges. SPI lets the application configure a driver and read its registers. Current and voltage signals return to the ADCs for feedback, while nFAULT inputs indicate driver faults.

The four drivers have separate chip-select signals but shared enable controls. This makes [channel selection and enable behavior](control.md) useful to understand before trying a motor-control application. The [sensing guide](sensing.md) follows the feedback path and explains how to convert readings into useful units.

## System boundary

For reusable controller features—UART, CAN, power selection, programming and recovery—use the <a href="../som/index.html">SoM manual</a>. For the battery, motor leads and FC harness, start with this manual's [connection guide](connections.md).
