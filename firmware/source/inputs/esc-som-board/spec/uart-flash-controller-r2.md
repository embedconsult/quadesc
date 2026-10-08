# Board flash custody r2

`Resources.flash` remains the unique NVMNW handle moved from `Peripherals`; no second MMIO handle or raw-address public API is added. Target settings storage is restricted to MAIN A `[0x78000,0x78800)` and B `[0x7c000,0x7c800)`, opposite bank0 code execution. Board startup leaves motor/CAN and hardware protections untouched. Actual mapping/static protection and complete write-enabled call/IRQ/stack closure are admission evidence, not inferred from linker load intervals.
