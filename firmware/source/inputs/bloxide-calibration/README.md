# bloxide-calibration

Portable, allocation-free calibration descriptors, validated scalar values,
stable virtual maps, and owner-mediated mutation contracts.

The runtime crate is `no_std` and has no Bloxide runtime, XCP, hardware, drone,
or measurement dependency. It implements the generic ownership defined by the
immutable S0 contracts; component repositories retain their own complete-value
and cross-field policy. The optional `std` feature provides owned deterministic
JSON exchange structures for host tools. Firmware uses checked compile-time
descriptors and does not parse JSON.

## Main contracts

- `BoundedU16`, `BoundedU32`, `BoundedI32`, and `define_bounded_f32!` create
  unit-aware values with private representations and checked byte decoding.
- `VariableDescriptor` and `Registry` reject invalid defaults/types/policies,
  duplicate identities, overlapping/overflowing regions, gaps, cross-region
  access, read-only writes, and incomplete scalar writes.
- `Owner` serializes one global mutation, retains terminal results, fences
  retired request sequences, represents cancel-before-apply tombstones, and
  preserves active state/outcomes during coordinated reset reconciliation.
- `CommitGate` requires bounded admission before changed state is committed.
  Its publication step is infallible; physical completion/fault status is a
  separate concern and cannot rewrite `Applied` as a rejection. Same-value
  assignments do not reserve output, increment revision, or restart effects.
- `Owner::apply(request, clock, make_candidate, gate)` takes a bounded,
  infallible `FnMut() -> u64` monotonic microsecond hook. Read the live clock
  inside the hook, in the deadline's time domain. The owner samples again after
  validation for a no-op or after reservation immediately before mutation.
  At/after expiry it cancels the unpublished permit and retains `Expired`;
  success uses the final sample in both the record and outcome. Component
  publication uses that record time for its phase epoch. Hook/gate/capture
  cleanup must obey the startup-only heap rule; borrow persistent resources.

The local API binding and regression map are in
[`specs/final-apply-clock.md`](specs/final-apply-clock.md).

## Scope boundaries

LED phase calculation/output-slot synchronization belongs in `bloxide-led`.
XCP address-to-wire behavior and A2L export belong in `bloxide-xcp`.
Read-only coherent snapshots and bounded acquisition storage belong in
`bloxide-measurement`. Persistence and physical I/O are separate capabilities.

Contract source: `contracts-s0-v0.1.1`, archive SHA-256
`f6f75d443d96dd762d3640e1acc76480243eabc88fd441574b4cad4f4f6705db`.
No full ASAM conformity or hardware validation is claimed.
