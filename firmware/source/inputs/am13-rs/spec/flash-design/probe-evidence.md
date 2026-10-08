# Flash analysis and verification scope

The retained layout and placement documents state the bank, slot, granule and
RAM execution assumptions for a generic portable persistence backend. Arithmetic
range checks can reject out-of-bank or overlapping loads, but cannot establish
transitive RAM closure, physical interruption/ECC behavior or durable wear policy.

`../../probes/flash_ram_closure.py` analyzes linked ARM ELF mappings and refuses
incomplete or misplaced RAM flash islands. Its result deliberately leaves
`ram_closure_proved=false`; transitive code, literals, handlers, stack and access
review remains necessary. This is read-only host analysis, with no device writes.

Current application and loader artifact hashes, ELF programmed-byte verification
and reproducible build results are in `firmware/build-report.json`. Generic host
models remain separate from the application's short physical bench evidence.
Internal task execution streams and historical diagnostic images are excluded.
