# Clock, GPIO and timing evidence scope

The board source uses SYSOSC32M, with MCLKDIV2 bypassed in SYSOSC mode. The UART
clock divider ratio zero means divide by one; these are register semantics from
TI SPRUJF2B section 3.4.1 and the vendor register definitions. Register-model
tests establish configured divider and pin values. Electrical baud, oscillator
error, timer intervals and LED transitions require measurements of the exact image.

PB18/package84 and PA0/PA1 are reviewed against the included CSV/SysConfig inputs.
The saved design gives active-low LED polarity: +3.3 V through R12 2.2 kΩ to D1
anode, with D1 cathode at PB18. GPIO tests cover thirteen PWM outputs, two enables,
four driver chip selects, LED and protected pin exclusions. A GPIO readback is
configuration evidence rather than optical confirmation.

SysTick has one pending bit; masking a 100 µs tick for 100 µs or longer can lose
time. Timing admission therefore depends on bounded service and interrupt
behavior. The current application and hardware measurements are described by
the engineering guide, with tested image hashes and scope channel mapping.
Historical diagnostic images and internal execution logs are outside this handoff.
