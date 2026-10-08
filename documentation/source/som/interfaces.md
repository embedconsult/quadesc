# Interface cheat sheet

Choose an interface here, then follow the [connector reference](connectors.md) to find its physical pins. MCU package pins and connector pad numbers use different numbering systems.

| Interface | MCU route / example mux | Connector access | Practical use |
|---|---|---|---|
| Application / ROM UART | UC4 TX PA0 pin 20; RX PA1 pin 21; mux 7 | J2.4 TX, J2.6 RX; J1 A3/C3 | Application UART or ROM BSL access, using the protocol for the active boot mode |
| Additional UART | UC1 TX PB21 pin 87; RX PB22 pin 88; mux 8 | J3.7 / J3.8 | Additional serial connection with application configuration |
| I²C | UC2 SDA PA22 pin 93; SCL PA23 pin 94; mux 4 | J4.3/4 and J3.10/9 | Shared two-wire bus; configure controller transactions with completion timeouts |
| SPI | UC3 MOSI PC6 pin 5; MISO PC4 pin 3; SCLK PC2 pin 1; mux 6 | J6.10/9/8 | Configure mode, word length and chip-select timing for the connected device |
| CAN controller | MCAN0 TX PA12 pin 73; RX PA11 pin 72; mux 10 | On-module U2, bus J5.2/3 and J6.3/2 | Application-configured CAN traffic through the on-module transceiver |
| Debug | PA13 SWDIO, PA14 SWCLK | J2.2 / J2.3 | Core access and programming through a compatible probe |
| Analog | 30 assigned pads across ADC0–2 | J1; AN on J6.5 is PA28 | Application-scheduled acquisition with the carrier-supplied reference |
| Timed output | MCPWM output routes on exposed pads | J1; PB12 also J3.5 | Continuous PWM; outputs on each module share a period |
| Expansion control | PB2 INT, PA20 reset-purpose GPIO, PA21 CS-purpose GPIO | J3.6, J6.6, J6.7 | GPIO connections for interrupt, reset or chip-select roles in the application |

## ROM transport is a separate capability

ROM CAN uses **1 Mbit/s nominal/data**, **CAN-FD without BRS**, and **IDs 3/4**. Follow the boot-clock and BOOT-pin setup in [boot and recovery](boot.md). Application CAN requires its own controller, clock and pad initialization.

## CAN termination control

**PC0, MCU pin 97**, controls the CAN termination switch through `CAN_TERM`. This signal is also available at **J1.B9**. Configure PC0 as a GPIO output to select termination for the module's position on the bus. PB9 is a separate GPIO at J1.K7.

## Multiplexing rules

A pad uses one selected digital function at a time. Choosing PWM on a pad replaces its GPIO function; analog inputs need the appropriate analog configuration. The [GPIO download](evidence.md) lists MCU package pins, board nets and connector access to help you follow each route.

The expansion rows each have 12 pins. J6.2/3 carry CANL/CANH, and J4 provides Qwiic access to I²C. Use the module's pinout when connecting accessories, including the power and ground pins.
