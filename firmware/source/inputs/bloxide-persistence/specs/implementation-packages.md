# Bounded implementation packages after independent design approval

Round-019 independently approved the portable I1-I4 scopes. They are implemented
on this branch and await product review. The package requirements below remain the
reviewed behavior; this status note does not relax them. I5-I7 remain gated and were
not implemented by this dispatch.

| Package / owner | Deliverables and exit |
|---|---|
| I1 T17 portable core | `bloxide-persistence` no_std crate, schema callback interface, checked codec/CRC and recovery classifier. No runtime/HAL/XCP dependency. Implement corrected R1-R3 and erase-integrity-proof.md: complementary body INCLUDING CRC, fixed logical/physical coordinate spaces, Q erase prestate classifier, independent golden vectors/decoder and directed codeword searches C01-C05/C14-C16. Q must not be replaced by an assumption that every Corrupt slot is erasable. No generated Rust edits. |
| I2 T17 service + simulator | Fixed-state command/completion machine, owned buffers, retained results/fences, conservative wear-authority seam. Independent byte/bit/prefix backend with separately written recovery oracle. QualifyErase scans before permit consumption; exercise both Q-safe and Q-unsafe independently invalid media, exact original F1 negative control, erased-marker restoration and repeated interruptions. Finite samples supplement the all-subsets proof. Implement R4-R9 and C06-C13/C17-C20. Core acceptance must not count the production parser as its sole oracle. |
| I3 T17 T07 adapter | Optional adapter crate depending on exact Git calibration c0a693a8811d920d05850cfc2ebcfd7956a2741b, no path override/active checkout. Owner-turn snapshot, fixed one/four-client broker. Public API tests including revision wrap and later RAM edit; no direct active mutation. |
| I4 T17 external reuse | Independent temporary Cargo workspace outside source, consuming this private repository at full immutable commit through Cargo Git, no path dependency. Thermostat schema, 100 save/reboot cycles plus C19 failure sweep. Lock dependencies and capture URL/rev/tree/manifest/Cargo lock hashes. `cargo tree` must show no drone, AM13, GPIO, XCP or runtime dependency for portable core. Do not create another repository without dispatch. |
| I5 T17/Bloxide integration | After reviewed T06 pin, messages are plain owned data, context free synchronous actions, pure TOML blox if an actor is useful, system.toml concrete adapter/backend wiring. Mailbox-free backend owns moved resource; no Reset reacquisition. Generate using pinned source tooling; real-action tests and T36 allocator instrumentation. |
| I6 P protocol owner | Resolve P-OPEN-1/2, review candidate vectors/metadata/error behavior, implement actual endpoint adapter at exact reviewed pins and independent unchanged pyXCP/pya2l tests C21-C23. S0 negative regression mandatory. No upstream client/parser patches or conformance claim. |
| I7 T16/T18 platform integration | T16 qualification of monotone erase/program subset model, coherent ECC/error visibility, slot/granule isolation, Q prestate stability, geometry/regions/RAM execution/timing/budget authority, separate reviewed target artifact release, native runner checks, MCU target builds and serialized UART reset/readback/power-interruption work. No device work from T17 server worker. |

Recommended dependency DAG: I1 -> I2 -> I3 -> I4; I5 requires I3+T06+T36;
I6 requires I3+P wire review and actual reviewed XCP endpoint; I7 requires I2+
T16+runner acceptance and separately authorized hardware procedure. I1-I4 are
useful without I5-I7 but cannot be reported as complete T17 product acceptance.

The F1-r2 design re-review released only I1-I4 portable implementation. No portable
result qualifies AM13 behavior, and no later hardware evidence may substitute for
the required model/physical qualification.

Use Rust1.94.0, CARGO_BUILD_JOBS=2 and a fresh owned /var/tmp NVMe scratch
CARGO_TARGET_DIR; record actual owner/filesystem and preserve final evidence in the
assigned hub. Use normal Cargo cache locks, no lock bypass/retry churn. Required
future commands include cargo test --locked for core+adapter, independent Git
consumer cargo test --locked, cargo check --locked --no-default-features --target
thumbv8m.main-none-eabi, and reviewed-source cargo blox test for integration.
The implementation evidence records the now-executed exact commands separately;
the original design evidence remains design-only.
Host x86_64, MCU thumb and native ARM64 results remain separately labeled.

## Interface handoff summary

Portable: Geometry + Slot IDs + Codec + owned Snapshot -> Save/Resolve/Release/
Quiesce -> owned one-at-a-time backend commands -> exact correlated completions ->
retained Durable/Failed/Indeterminate. Durable view is separately readable.
All values/sizes/fences live in record-and-service-v1.md R1-R9; exact proposed P
wire boundary is profile-p-proposal.md. Next implementation starts with independent
fixtures in tests/test-catalogue.json, not by translating this design checker
into the product simulator. Future readback test must compare canonical bytes,
not just CRC, RAM revision, success status or process exit.
