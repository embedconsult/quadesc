# Identification and mechanical reference

The AM13E SoM is a **25.4 × 38.1 mm** controller module built around the **XAM13E23019GTPZ** 100-pin MCU. U2 is the **TCAN844DRBRQ1** CAN transceiver, and U3 switches bus termination. U4, U5 and U6 handle power selection, load switching and digital regulation.

## Connector orientation

```{image} ../shared/assets/som-board-top.svg
:alt: AM13E SoM top assembly and connector locator
:target: _images/som-board-top.svg
```

```{image} ../shared/assets/som-board-bottom.svg
:alt: AM13E SoM underside assembly and connector locator
:target: _images/som-board-bottom.svg
```

Use these assembly views to find connectors and components. Reference labels match the KiCad board files. Select an image for a closer look.

**Top** looks down at F.Cu. **Bottom** looks directly at B.Cu after turning the board left-to-right, keeping the same top edge up. Left and right therefore reverse; the bottom drawing is not a view through the board. Use the [connector pad table](connectors.md) for electrical assignments.

| Connector | Purpose |
|---|---|
| J1 | Underside 100-pad LGA interface for a carrier |
| J2 | Eight-pin debug, UART, reset and boot row |
| J3 / J6 | J3 left / J6 right in the top view; 12 pins each |
| J4 | Qwiic I²C connection |
| J5 | CAN bus and power |

The nominal PCB outline has a southwest relief. Use the KiCad board outline when checking mating geometry and assembly clearance.

## Mechanical integration

J1 uses **1.27 mm pitch** with **0.6 mm pads**. The two expansion-row origins are **22.86 mm** apart. Successive row pad centers advance by **2.54 mm**, with alternating lateral stagger. Match the complete 12-pin row geometry when designing a mating connector.

Allow space for connectors, cables and assembled components as well as the bare PCB outline. For a soldered carrier, the [assembly notes](maintenance.md#assembly-and-service) explain the module stencil and mating-array paste arrangement. Keep the debug row accessible if you expect to program or service the module after installation.
