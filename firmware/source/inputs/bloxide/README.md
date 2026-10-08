# Bloxide framework source

Bloxide provides hierarchical state machines, actor messages, supervision,
capabilities and code generation. This handoff includes the pinned framework
source required by the drone board application and its regression fixtures.
The active application uses static Embassy wiring on the AM13 Cortex-M33.

Authored state topology and wiring live in `blox.toml` and `system.toml` files;
`cargo-blox` materializes Rust crates and application scaffolding as disposable
build outputs. Plain messages and context action functions retain their separate
ownership boundaries. The framework's reusable Tokio and test runtime APIs remain
available in source, with no alternate customer application selected.

Read the [architecture and invariants](spec/README.md),
[layered architecture](spec/architecture/00-layered-architecture.md),
[static wiring](spec/architecture/03-static-wiring.md),
[declarative wiring](spec/architecture/14-declarative-wiring.md),
[generation source of truth](spec/architecture/15-blox-toml-source-of-truth.md),
and [embedded generation](spec/architecture/19-embedded-generation.md).
The [framework API reference](skills/building-with-bloxide/reference.md) gives
the reusable API details; its illustrative snippets are not additional delivered demos.

Use the customer [firmware build/test instructions](../../../../tools/firmware/README.md)
for pinned complete application generation, builds and tests. Framework sources
and local paths are selected by the application workspace and locks. Current
component identity and per-file hashes are in
[`firmware/source-provenance.json`](../../../source-provenance.json).
Existing [MIT license](LICENSE) and source notices are retained.
