# Connector and pad reference

Use these tables to wire external connectors or trace a signal through the installed SoM interface. Pad numbers and net names follow the KiCad PCB. Each row represents one electrical pad number on one connector. Check the connector locator and key before wiring: a cable-face view can reverse the apparent pin order.

## J4 — Motor 3 phases

Board side: **F.Cu**. Use the board views and connector key for orientation.

| Pad | PCB net |
| --- | --- |
| 1 | `M3_PHASEA` |
| 2 | `M3_PHASEB` |
| 3 | `M3_PHASEC` |

## J5 — Motor 4 phases

Board side: **F.Cu**. Use the board views and connector key for orientation.

| Pad | PCB net |
| --- | --- |
| 1 | `M4_PHASEA` |
| 2 | `M4_PHASEB` |
| 3 | `M4_PHASEC` |

## J8 — buzzer

Board side: **F.Cu**. Use the board views and connector key for orientation.

| Pad | PCB net |
| --- | --- |
| 1 | `GND` |
| 2 | `BUZZER` |

## J2 — Motor 1 phases

Board side: **F.Cu**. Use the board views and connector key for orientation.

| Pad | PCB net |
| --- | --- |
| 1 | `M1_PHASEA` |
| 2 | `M1_PHASEB` |
| 3 | `M1_PHASEC` |

## J6 — FC interface

Board side: **F.Cu**. Use the board views and connector key for orientation.

| Pad | PCB net |
| --- | --- |
| 1 | `GND` |
| 2 | `+BATT` |
| 3 | `FC_Current_IO` |
| 4 | `FC_Telemetry` |
| 5 | `DSHOT1` |
| 6 | `DSHOT2` |
| 7 | `DSHOT3` |
| 8 | `DSHOT4` |
| P1 | `no assigned net` |
| P2 | `no assigned net` |

## J3 — Motor 2 phases

Board side: **F.Cu**. Use the board views and connector key for orientation.

| Pad | PCB net |
| --- | --- |
| 1 | `M2_PHASEA` |
| 2 | `M2_PHASEB` |
| 3 | `M2_PHASEC` |

## J7 — debug

Board side: **F.Cu**. Use the board views and connector key for orientation.

| Pad | PCB net |
| --- | --- |
| 1 | `+3.3V` |
| 2 | `SWDIO` |
| 3 | `GND` |
| 4 | `SWCLK` |
| 5 | `GND` |
| 6 | `SWO` |
| 7 | `unconnected-(J7-Pin_7-Pad7)` |
| 8 | `TDI` |
| 9 | `GND` |
| 10 | `nRST` |

## J9 — SoM landing

Board side: **B.Cu**. Use the board views and connector key for orientation.

| Pad | PCB net |
| --- | --- |
| 1 | `GND` |
| 2 | `GND` |
| 3 | `GND` |
| 4 | `GND` |
| 5 | `+3.3V` |
| A1 | `DSHOT2` |
| A2 | `M4_IC` |
| A3 | `unconnected-(J9-PadA3)` |
| A4 | `M4_BEMFA` |
| A5 | `nFAULTM4` |
| A6 | `M2_PWMA` |
| A7 | `M3_PWMA` |
| A8 | `SPI_nCSM4` |
| A9 | `M4_PWMB` |
| A10 | `M4_PWMA` |
| B1 | `unconnected-(J9-PadB1)` |
| B2 | `M_TEMP2` |
| B3 | `M4_IA` |
| B4 | `M2_PWMC` |
| B5 | `M3_BEMFB` |
| B6 | `M2_PWMB` |
| B7 | `SPI_nCSM2` |
| B8 | `SPI_MISO` |
| B9 | `unconnected-(J9-PadB9)` |
| B10 | `M3_PWMC` |
| C1 | `M2_IA` |
| C2 | `M2_BEMFC` |
| C3 | `unconnected-(J9-PadC3)` |
| C4 | `unconnected-(J9-PadC4)` |
| C5 | `nRST` |
| C6 | `nFAULTM2` |
| C7 | `SPI_MOSI` |
| C8 | `SPI_SCLK` |
| C9 | `M4_PWMC` |
| C10 | `unconnected-(J9-PadC10)` |
| D1 | `M2_IB` |
| D2 | `M2_BEMFB` |
| D3 | `M2_IC` |
| D4 | `GND` |
| D5 | `BUCK_OUT` |
| D6 | `GND` |
| D7 | `BUCK_OUT` |
| D8 | `unconnected-(J9-PadD8)` |
| D9 | `unconnected-(J9-PadD9)` |
| D10 | `unconnected-(J9-PadD10)` |
| E1 | `M_TEMP4` |
| E2 | `M4_BEMFC` |
| E3 | `M2_BEMFA` |
| E4 | `BUCK_OUT` |
| E5 | `GND` |
| E6 | `BUCK_OUT` |
| E7 | `GND` |
| E8 | `unconnected-(J9-PadE8)` |
| E9 | `SWO` |
| E10 | `unconnected-(J9-PadE10)` |
| F1 | `M_TEMP1` |
| F2 | `M1_BEMFB` |
| F3 | `M1_BEMFC` |
| F4 | `GND` |
| F5 | `VAA` |
| F6 | `GND` |
| F7 | `BUCK_OUT` |
| F8 | `FC_Telemetry` |
| F9 | `BUZZER` |
| F10 | `unconnected-(J9-PadF10)` |
| G1 | `M1_IB` |
| G2 | `SPI_nCSM1` |
| G3 | `M_TEMP3` |
| G4 | `BUCK_OUT` |
| G5 | `GND` |
| G6 | `BUCK_OUT` |
| G7 | `GND` |
| G8 | `DRV_ENABLE` |
| G9 | `unconnected-(J9-PadG9)` |
| G10 | `nFAULTM3` |
| H1 | `M1_IA` |
| H2 | `M1_BEMFA` |
| H3 | `DSHOT1` |
| H4 | `M3_IC` |
| H5 | `M3_BEMFC` |
| H6 | `M1_PWMA` |
| H7 | `SPI_nCSM3` |
| H8 | `unconnected-(J9-PadH8)` |
| H9 | `FC_Current` |
| H10 | `INL_ENABLE` |
| J1 | `DSHOT4` |
| J2 | `M1_IC` |
| J3 | `unconnected-(J9-PadJ3)` |
| J4 | `DSHOT3` |
| J5 | `VBAT_SNS` |
| J6 | `M1_PWMC` |
| J7 | `M4_IB` |
| J8 | `M3_PWMB` |
| J9 | `SWCLK` |
| J10 | `TDI` |
| K1 | `unconnected-(J9-PadK1)` |
| K2 | `nFAULTM1` |
| K3 | `M3_IA` |
| K4 | `M3_BEMFA` |
| K5 | `M3_IB` |
| K6 | `M1_PWMB` |
| K7 | `unconnected-(J9-PadK7)` |
| K8 | `M4_BEMFB` |
| K9 | `unconnected-(J9-PadK9)` |
| K10 | `SWDIO` |

## J1 — battery entry

Board side: **B.Cu**. Use the board views and connector key for orientation.

| Pad | PCB net |
| --- | --- |
| 1 | `+BATT` |
| 2 | `GND` |

## Using the pad table

The [top and underside board views](overview.md) show the viewing convention.

Each battery or motor solder terminal appears once. Its smaller stitching pads share that terminal’s pad number and net; they are not extra pins. J1 has two power terminals, and J2–J5 each have three phase terminals. The [connection guide](connections.md) groups them by use.

The downloadable CSV contains the same unique pad assignments and board sides. Separate connectors and different pad numbers remain separate even when they share a net. A net marked `unconnected` has no signal connection at that pad. Mechanical pads appear too, since some connect to power or ground.

{download}`Download pad CSV <../../reference/mainboard-connector-pads.csv>`
