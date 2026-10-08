# am13-rs

Small all-Rust `no_std` AM13E23019 support extracted from the frozen corrected diagnostic firmware. This is chip support, not a complete PAC, an ESC application, or electrically qualified new firmware. SoM pin assignments live in `esc-som-board`.

`Peripherals::take()` is a one-shot hardware acquisition that quiesces inherited IRQs. It yields independent clock, GPIO setup, UC4 and SysTick owners. Handles remain non-Send/non-Sync and move into a single thread-mode executor; no legacy global MMIO reference or mutable actor backdoor is exposed. Drop never releases the singleton or reinitializes hardware.

Clocks select nominal 32MHz SYSOSC. `Clocks::uc4_hz()` correctly reports 32MHz with the MCLKDIV2 domain bypass; UART uses CLKSEL=8 and CLKDIV=0. `Uc4::configure` prepares 8N1 disabled, the board selects pads, and `enable` verifies readback. `Uart::try_read/try_write/is_tx_complete` do bounded register work; `embassy` adds embedded-io-async Read/Write with 100us timer retries and cancellation-safe single-byte admissions. TX admission waits for the previous byte’s EOT; flush includes the final stop bit and is independent of receive activity. See [TX completion and bounds](spec/uart-completion.md). GPIO outputs implement embedded-hal 1.0.0. Readback is register/pad evidence, never optical observation.

The `embassy` feature provides the real Embassy time driver and sole SysTick handler. It uses a coherent u64 10kHz counter and 16 fixed task/deadline slots. It validates Embassy task wakers and stores TaskRef, so no generic Waker clone/drop or timer allocation occurs. Cancelled timers can leave one earliest pending deadline per task until expiry and cause harmless early wakes. A seventeenth retained task deadline latches `time::capacity_faulted()` and panics; target integration must use an allocation-free halt handler. See [platform contract](spec/platform.md) and [integration obligations](spec/integration.md).

Build/test on Rust 1.94.0:

```sh
CARGO_BUILD_JOBS=4 cargo test --locked --features embassy
CARGO_BUILD_JOBS=4 cargo clippy --locked --all-targets --features embassy -- -D warnings
CARGO_BUILD_JOBS=4 cargo check --locked --target thumbv8m.main-none-eabi --features embassy
```

Set `CARGO_TARGET_DIR` to a job-local directory. The linked target example is in the independent `esc-som-board` consumer. No C SDK, ROM FFI, heap, CAN, flash/security writes, PLL, SRAM3 configuration, or diagnostic monitor is included.

Tests include register faults, ownership, clock/divisor traces, u64 carry, full timer table, cancellation and real Embassy executor first-poll/timeout/expiry/task-cleanup plus async UART. A test instruments all four allocator calls, proves detection by deliberate calls, then observes zero attempts for the selected executor/driver workload. This is x86_64 driver evidence; T36 and full-image T13/T35 remain independent allocation/latency gates.

Source notices/provenance are recorded in [PROVENANCE.md](PROVENANCE.md); no license has been invented. Origin is private.
