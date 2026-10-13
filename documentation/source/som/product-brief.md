# BeagleConnect Torque product brief

## Product description

BeagleConnect Torque is a compact processor module for motor control, motion control and CAN-based sensing, actuation and display designs. It combines a TI AM13E23019 200 MHz Arm Cortex-M33 microcontroller with 512 KB flash, 128 KB SRAM, a TinyEngine neural-network accelerator, a trigonometric math unit and hardware security capabilities.

Torque is the central controller in BeagleConnect QuadESC, a four-channel brushless DC motor electronic speed controller. The standalone solder-down module is intended for custom carriers, rapid prototyping and production designs. The motor gate drivers and power transistors are on the QuadESC carrier, not on Torque.

The design is developed in the USA, with assembly in Michigan as specified for the product. Editable hardware designs and firmware sources are available in the [QuadESC repository](https://github.com/embedconsult/quadesc). Production origin, compliance, certification and ordering details require release confirmation.

## Applications

- Drone electronic speed controllers, including the four-channel QuadESC carrier.
- Robot joints and wheel drives, gimbals, actuators, positioning stages, pumps and fans.
- Field-oriented control (FOC), sensorless control and custom motion algorithms with suitable application firmware and external power stages.
- Local neural-network inference for motor fault and anomaly detection with application-specific models and firmware.
- CAN-FD sensor, actuator, indicator, power and protocol-bridge nodes; DroneCAN requires application firmware.
- Education and research in motor control, embedded communications and edge AI.

## Development and production

Load custom firmware through **Serial Wire Debug (SWD)** or the TI ROM bootstrap loader (BSL) over CAN, I²C or UART. The module ships with **TI ROM BSL only**. ROM CAN settings and boot configuration are documented in [boot and recovery](boot.md); application protocols configure their own clocks and interfaces.

Support targets community-aligned mainline Zephyr and OpenOCD. Validate board support and programmer compatibility for each release. Hardware capability alone does not establish released DroneCAN, FOC or motor-diagnostics firmware.

Use the underside LGA interface to integrate Torque into a custom carrier. BeagleBoard.org Foundation and its community provide an engagement path, with professional hardware and software consulting available through the ecosystem. Open source hardware certification and its identifier require confirmation.

## Key features and specifications

| Item | Value |
|---|---|
| MCU | TI AM13E23019, Arm Cortex-M33, 200 MHz; current PCB U1 is XAM13E23019GTPZ in a 100-pin package |
| Memory | 512 KB flash; 128 KB SRAM |
| Acceleration | TinyEngine NPU, trigonometric math unit and floating-point unit |
| Security | MCU secure boot, secure debug, secure firmware update and cryptographic acceleration; application configuration and provisioning required |
| Motor control | Controller for four three-phase motor channels on QuadESC; external power stages and application firmware required |
| CAN hardware | U2 TCAN844DRBRQ1 transceiver; J5 four-pin JST-GH connector: 1 +5V, 2 CANH, 3 CANL, 4 GND |
| CAN termination | R10 120 Ω resistor switched by U3 TS5A3166YZPR; MCU PC0 / `CAN_TERM`, also available at J1.B9 |
| Qwiic | J4: ground, 3.3 V, I²C SDA and SCL |
| Expansion | Two 12-pin rows, J3/J6, exposing UART, SPI, analog, PWM and mikroBUS-related control signals; use the actual module pinout for compatibility |
| Analog | 30 assigned ADC pads; external VAA/VREFHI reference required |
| Debug and boot | J2 eight-pin row: SWDIO, SWCLK, UART TX/RX, reset, boot invoke, ground and 3.3 V debug input |
| Carrier power | Nominal regulated 5 V at `+VDC`, through TPS22950 load switch and TPS7A2633 regulator; no raw battery connection |
| Debug power | Separate 3.3 V input at J2.1, selected by TPS2116 mux; CAN still requires its 5 V rail |
| Supply qualification | Module input tolerance, accessory current budget and operating temperature require assembled-board qualification |
| Mounting | 100-pad underside LGA, 1.27 mm pitch, 0.6 mm pads |
| PCB outline | 25.4 mm width × 38.1 mm length, with a southwest relief |
| PCB stackup | Four-layer FR-4; 0.035 mm copper per layer (nominal 1 oz); 1.6 mm PCB thickness |
| Assembled dimensions | Component/cable clearance in addition to the PCB outline; total height and net weight to be measured |
| Firmware | TI ROM BSL at shipment; load application through CAN, I²C, UART or SWD |

The connector supply rails share their capacity with onboard loads. See [power](power.md) for digital and analog supply paths and [carrier integration](carrier.md) before designing a mating board. PCB thickness is not assembled module height.

## Physical data, compliance and packaging

| Field | Status |
|---|---|
| Product name | BeagleConnect Torque |
| PCB length / width | 38.1 / 25.4 mm |
| Net weight / assembled height | Measure production unit |
| Country of origin | USA design and Michigan assembly stated; final production origin audit pending |
| NDAA | Production BOM and supplier compliance audit required |
| RoHS / REACH | Planned; declarations and production verification pending |
| Open source hardware certification | Certification and identifier to be confirmed |
| Standalone package | One Torque module; static-shielding bag and final packaging to be confirmed |
| Product configurations | Standalone module or integrated QuadESC controller; release ordering details to be confirmed |

## Board views

The [top and underside assembly views](identity.md) identify components, connectors and mounting geometry. They are design illustrations, not production photographs. Use the editable PCB outline and full mating footprint for manufacturing.

## Sources and release checks

Hardware values above come from `som/som.kicad_pcb` and `som/mcu-core.kicad_sch`, `som/power.kicad_sch`, `som/can-transceiver.kicad_sch` and `som/io-connectors.kicad_sch`, at repository commit `1fe0bbf0fa20e1aa848fa81a7b839ce0438ad440`. The [connector reference](connectors.md) and [interface guide](interfaces.md) provide electrical mappings.

MCU capabilities are specified by the [TI AM13E23019 product page](https://www.ti.com/product/AM13E23019) and [AM13E230x datasheet](https://www.ti.com/lit/ds/sprspc3/sprspc3.pdf). MCU features and ratings do not establish the finished module's qualification or software release status.

The current PCB/schematics and `som/som_bom_with_mpns.csv` use differing component and connector references. Reconcile the production BOM before procurement or compliance signoff. Confirm the production MCU ordering code, module supply/current/temperature limits, unit weight and height, packaging, certification, and released firmware/tool support before publishing final production claims.
