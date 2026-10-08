# SoM LED-cal profile 0.1

Pin source: frozen current DRONE_ESC_v3.syscfg and summary.csv, hashes recorded in provenance. PB18/package84 is STATUS_LED; active low is corroborated by corrected source's saved schematic evidence (D1 cathode to PB18), not newly measured assembly evidence. PA0/PA1 mux7 UC4 remain stock BSL UART. Never configure CAN/debug pads or boot/security flash.

Board initialization once per MCU boot: quiesce inherited IRQs; latch all enable/PWM outputs low and CS high before enabling GPIO mux; LED off; input BSL_INVOKE pull-down. Select/verify SYSOSC; configure UC4 disabled, mux UART, enable/verify UC4. Consume GPIO setup into sole LED output. Keep unexposed motor pins owned by board lifetime. No actor reset reacquires these resources. Failures return StartupError to a non-allocating halt policy, with no actor admission.

The application selects UC4 baud at startup through `initialize(device, uart_baud)`. The board example retains candidate 38400 8N1 and initial SysTick100us; the current diagnostic fixture requires 19200 when integrated with this board API. Historical diagnostic profile core0.1.2 was separately measured at 19200/~19236.13 baud and 1ms tick. The linked example is a platform qualification fixture, not generated final LED/XCP firmware. No diagnostics or text monitor added. A downstream firmware selects its linker script, single executor, critical-section implementation and panic behavior; example uses 512KiB FLASH and initial 96KiB RAM without SRAM3 mux changes.

Acceptance tests: register model enforces GPIO atomic set/clear and verifies actual readback/mux, safe ordering, all 13 PWM pads inactive; no CAN/debug/flash accesses. Cross-link actual Embassy timer/LED/UART tasks with one reset/vector/time driver. Hardware gate repeats known BSL recovery, measures UART and 100us timebase/LED under traffic and separates GPIO/optical evidence. Current no5V fixture prohibits CAN.

The moved UART uses the chip’s `is_tx_complete`/async flush contract: untouched TX is complete, and admitted data completes through final stop-bit EOT independently of reception. Admission allows one outstanding byte; service code must handle would-block even with FIFO space. The paced echo fixture explicitly flushes each reply to link and exercise the EOT adapter. This baud selection changes no board clock or pin assignment; simultaneous RX/TX throughput and latency still require hardware qualification.
# Flash resource handoff (2026-09-25)

The board initializer consumes the unique chip `Peripherals.flash` handle
alongside LED/UC4/SysTick and hands it to firmware. It performs no flash MMIO
or write admission. Firmware must retain this handle through logical resets.
