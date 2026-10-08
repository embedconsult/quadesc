# Board peripheral chip support

These are bounded polling drivers with exclusive handles from `Peripherals::take`. They use 32 MHz SYSOSC/MCLK except MCAN, which explicitly starts the 25 MHz XTAL HFCLK and selects it as CANCLK. No driver allocates. `RegisterIo::wait` is bounded to 100,000 reads; I2C and CAN use similarly bounded foreground loops. The caller owns task scheduling and electrical validation.

| Driver | Register source | Scope |
|---|---|---|
| `pwm::McPwms` | SDK `hw_mcpwm.h`, `dl_mcpwm.h` | MCPWM0–3, one shared frequency per module, six output selectors, duty/high width, continuous start and forced-low stop. Tick/divider quantization returned. MCPWM offers action-qualifier one-shot **force level**, not an exact one-pulse waveform or N-period train. Exact finite trains would require a separate event/interrupt mechanism with verified latency, so no finite-count API is exposed. |
| `adc::Adcs` | SDK `dl_adc`, `hw_adc.h`; corrected diagnostic `chip/adc.rs` | Three ADCs, preserved ROM trim (power only, no reset), 16 MHz ADCCLK, external board reference, SOC0/SEQ4, 448-cycle sample window, raw 12-bit sequential conversion. Timeout is explicit. |
| `spi::Uc3` | SDK `hw_unicommspi.h`, `dl_unicommspi.h`; corrected diagnostic UC3 setup | 100 kHz mode 1, 16-bit MSB-first full-duplex word. Chip-select ownership is board-level. DRV8323 framing uses its TI datasheet: read bit 15, four address bits 14..11 and 11 data bits. |
| `i2c::Uc2` | SDK `hw_unicommi2cc.h`, `dl_unicommi2cc.h` | 100 kHz 7-bit controller, 1..4095 byte write/read and write-then-read repeated START, NACK/arbitration/error/timeout reporting. UC2 is the SDK's advanced-feature instance. External pull-ups and target response still require a bench check. |
| `can::Mcan0` | SDK `hw_mcan.h`, `dl_mcan.c`, `hw_sysctl.h`, `dl_sysctl.c`; TI AM13E230x TRM clock tree | 125/250/500 kbps nominal from 25 MHz XTAL; classic standard-ID data frames, one RX FIFO0 element and one dedicated TX buffer, 8-byte payloads. No CAN-FD, extended ID, remote frame or protocol stack. TX completion is polled and cancellation requested on timeout. Board transceiver and XTAL operation are unverified electrically. |

The local SDK copy is register and sequence reference; no TI source was copied into the Rust files. The SDK's existing notices stay with those source files. The board pin authority is the current v3 SysConfig export in the companion crate, rather than historical spec tables.
