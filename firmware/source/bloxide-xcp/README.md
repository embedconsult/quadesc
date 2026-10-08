# bloxide-xcp

The drone board composition in `../drone-esc-firmware/examples/am13-xcp-control/system.toml` selects this package's CLI-generated Session actor. Ready/connection/recovery/waiting leaves route packets declaratively, and fixed bridge slots carry owned provider commands and complete responses. The independent S0 packet/core/SxI libraries remain reusable and no generated Rust is edited in source.

Reusable transport-neutral XCP protocol surfaces for Bloxide applications.
The implemented `0.1.0` cut is the deliberately narrow scalar S0 profile:
XCP 1.0/SxI 1.0 identifiers, CTO/DTO 8, little-endian byte addressing, and
only CONNECT, DISCONNECT, GET_STATUS, SYNCH, SET_MTA, UPLOAD, and DOWNLOAD.

This is a scoped interoperability profile, not a full ASAM XCP conformity
claim. DAQ, pages, STIM, programming, security, block mode, discovery, CAN
framing, and A2L IF_DATA are not implemented or advertised.

## Packages

- `xcp-messages`: fixed-size plain packet and provider-completion data.
- `xcp-core`: `no_std`, allocation-free codec/session/map/provider boundary.
- `xcp-sxi`: `no_std`, allocation-free LEN8/CTR8/SUM8 codec, explicit-idle
  parser and bounded Session adapter.
- `blox-ctx-xcp`: synchronous context actions with one retained effect and
  one retained response slot.
- `bloxes/session/blox.toml`: pure declarative broker topology; generated Rust
  remains build output.
- `fixtures/generated-session`: real concrete generated actions and lifecycle/
  correlation/saturation regressions against pinned Bloxide, plus a mock endpoint.
- `fixtures/standalone-consumer`: unrelated thermostat-style downstream
  consumer proving the core has no drone, LED, AM13, runtime, or HAL coupling.
- `fixtures/sxi-endpoint`: host-only real Session/SxI endpoint with a bounded
  deterministic provider fake for PTY qualification.
- `interop`: accepted T10 owned synchronous pyXCP transport plus the T34 product
  endpoint runner. The integrated T10 subtree identity is locked in
  `integration-lock.json`.

The checked virtual map resolves logical addresses to descriptor IDs and
offsets. It never turns a client address into a Rust pointer. DOWNLOAD emits
an owned provider request and responds positively only after the provider
reports the canonical owner outcome as Applied. Retired, stale, lost, or
uncertain outcomes fence the session rather than fabricate a rejection.

See [spec/xcp-s0.md](spec/xcp-s0.md) for exact PDUs and invariants and
[application source overview](../drone-esc-firmware/README.md) for the current integration boundaries.

## SxI ownership and scheduling

Construct the adapter once with `xcp_sxi::with_adapter(|adapter| ...)` and move
it into its owning service inside that scope. Lease lifetimes belong to that
instance; completion consumes the opaque token. Check `lease_is_current` before
bounded emission, then `complete_tx`. Use `lifecycle_fence` for Stop/Reset, retaining
Session truth and resources. There is no clone/default/reconstruction API.

Core `Session::service` returns `ServiceResult` with durable dispatch and per-call
new-fault provenance; SxI uses both to revoke partial input on fresh provider
failure while preserving Busy progress.

Call `service` at bounded control opportunities even under RX/TX pressure; supply
monotonic timestamps to parser polling and pre-arrival feeding. Late suffixes
expire before timestamp refresh at >=20ms. `observe_idle(now, provider)` is the
explicit recovery operation and durably fences clock faults. See
[spec/sxi-s0.md](spec/sxi-s0.md) for the complete scheduler and ownership contract.

## Development

Rust 1.94.0 is pinned by `rust-toolchain.toml`.

```sh
CARGO_BUILD_JOBS=2 cargo test --workspace --locked
CARGO_BUILD_JOBS=2 cargo clippy --workspace --all-targets --locked -- -D warnings
CARGO_BUILD_JOBS=2 cargo check -p xcp-core -p xcp-sxi \
  --target thumbv8m.main-none-eabi --locked
CARGO_BUILD_JOBS=2 cargo test \
  --manifest-path fixtures/standalone-consumer/Cargo.toml --locked
CARGO_BUILD_JOBS=2 cargo test \
  --manifest-path fixtures/generated-session/Cargo.toml --locked
CARGO_BUILD_JOBS=2 cargo build \
  --manifest-path fixtures/sxi-endpoint/Cargo.toml --locked
```

The accepted Bloxide baseline `43c4faa...` can lint the source topology. Its
pre-package code generator requires framework crates and the codegen crate as
local paths inside the consumer workspace, so full external materialization
uses the package/composition work owned by T06. This repository does not copy
framework sources or commit generated Rust to work around that boundary.

Public reference pins used for the selected command/error layouts:

- pyXCP 0.29.18 commit
  [`016cf3e`](https://github.com/christoph2/pyxcp/tree/016cf3e44364e9cd93966d144a39d342578a0391)
- [ASAM XCP public overview](https://www.asam.net/standards/detail/mcd-1-xcp/wiki/)

The retained interoperability fixtures exercise protocol packet ownership and
framing independently of the hardware. Their source contracts do not constitute
electrical UART/CAN timing or ASAM conformance evidence. Current application and
host regression results are recorded in `firmware/build-report.json`.

## Restricted persistence profile P (reusable source)

[spec/profile-p.md](spec/profile-p.md) documents the separate reviewed single-page P adapter, its borrowed Owner/service seam, matched host PTY fixture and remaining generated/target gates. S0 remains unchanged; the P adapter is selected explicitly by composition.

The LED integration branch resolves this crate's path dependencies to the selected local framework `2b4ec83`, calibration `c0a693a` and persistence `5d67c99` inputs. `tools/check_profile_p_inputs.py` checks those local pins and unchanged copied metadata. The firmware's generated owner action supplies Profile P's synchronous `Domain` borrow; this crate still has no second LED owner and its S0 source is unchanged. The application uses its own image identity and concrete board calibration backend; see the customer engineering manual for that integration.
