# Care and service

Before replacing a module or changing a carrier connection, disconnect power and check the mating orientation, pinout and supply paths. The [connector reference](connectors.md) and [power diagram](power.md) are useful companions at the workbench.

## Firmware compatibility

A replacement module needs an application suited to its connected devices. Use [programming an application](firmware.md) for image layout and loading, and [boot and recovery](boot.md) for programming connections.

If a replacement module powers up but does not answer over UART, check the baud rate and boot mode before changing the hardware. [Troubleshooting](troubleshooting.md) covers the common checks.

## Assembly and service

The SoM front stencil is **100 µm**, with a **75 µm local step at U5**. When printing paste on the carrier's mating array, leave the SoM bottom LGA unprinted so the joint receives paste from one side. U2 and U6 thermal apertures are notched around unfilled vias.

After rework, inspect nearby joints and check continuity before applying power. Hidden LGA joints need an inspection method suited to the package. Then check the digital rail, VAA if used, and the interfaces affected by the repair before reconnecting external loads.

## Getting help

A short description of the symptom and the connection you are using is a good starting point. For a UART issue, the baud rate and a few lines of terminal output help explain what happened. For an electrical issue, describe the supply and where you measured. If a component number could refer to either board, say whether it is on the SoM or the carrier.

## Glossary

| Term | Meaning |
|---|---|
| SoM | System-on-module; the reusable controller assembly |
| Carrier | Board providing power, connectors and application circuitry |
| BSP / HAL | Board support / chip-level hardware abstraction code |
| Mux | Selection of one alternate function on a physical pad |
| BSL | MCU bootloader interface |
| VAA / VREFHI | Analog supply net / ADC high-reference net |
| LGA | Land grid array used for the underside solder interface |
