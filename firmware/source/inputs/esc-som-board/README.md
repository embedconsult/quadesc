# esc-som-board

Owned board resources for the standalone AM13E23019 ESC SoM, using current SysConfig v3 pin authority. The original minimal LED-cal constructor establishes inactive enable/PWM GPIO, inactive driver chip selects, PB18 LED off, fault/DShot/BSL inputs, 32MHz SYSOSC and UC4 PA0/PA1 at the application-selected checked baud. It leaves ADC/SPI/I2C/CAN untouched. The explicit [board peripheral profile](spec/board-peripherals.md) adds the current v3 PWM, 30 raw ADC inputs, external DRV8323 SPI, UC2 I²C, MCAN0 and switchable CAN termination resources. It leaves driver enable low and CAN unconfigured for application-controlled startup; `examples/peripheral-resources.rs` links the profile on Cortex-M.

`initialize(Peripherals, uart_baud)` consumes setup once and returns movable `led`, `uart`, `systick`, and immutable `clocks` values. LED is active low; `set_on(true)` writes PB18 low and verifies GPIO/pad readback. `embedded-hal::OutputPin` retains physical-level semantics (`set_low` means LED on). No resource is cloned into services and actor Reset does not reconstruct the board. The future diagnostic integration must supply 19,200 or 38,400 for the earlier candidate example; invalid rates return `StartupError::Uart(Baud)`. Application ownership, calibration and output slots remain in firmware/feature crates.

Clone this private repository beside `am13-rs` at the revision in `source-lock.json`. The explicit path dependency is intentional for the accepted already-fetched composition slice; ordinary Cargo.lock pins external dependencies, and source-lock pins the sibling content. No transitive auto-fetch/codegen functionality is implied.

```sh
CARGO_BUILD_JOBS=2 cargo test --locked
CARGO_BUILD_JOBS=2 cargo build --locked --release --target thumbv8m.main-none-eabi --features firmware --example led-uart
```

The linked example runs actual Embassy timer tasks: a 500ms LED half-period and paced UART echo with bounded work per poll. It is a review/qualification fixture, not final generated LED/XCP firmware and not a historical text monitor. It has no allocator, uses cortex-m-rt Rust startup, 96KiB initial RAM, and 512KiB flash. The build script provides its memory map only to the firmware example; production firmware owns its layout. Set a job-local CARGO_TARGET_DIR.

See [profile](spec/led-profile.md), [timing evidence](spec/timing-evidence.md), [chip integration obligations](../am13-rs/spec/integration.md), and [provenance](PROVENANCE.md). Cross-build and host tests do not establish new electrical/optical or ARM64 execution claims. No devices were operated by this server worker.
