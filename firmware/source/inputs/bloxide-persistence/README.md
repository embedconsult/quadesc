# Bloxide persistence source

This package provides bounded calibration records, dual-record recovery,
immutable save snapshots and an owned-command persistence service. The portable
core and simulator are retained with their technical contracts, regression
vectors and source notices. The drone application's concrete calibration storage
implementation is selected separately by its board composition.

Read the [portable correction API](specs/portable-correction-r1.md),
[record and service contract](specs/record-and-service-v1.md),
[conditional erase/program proof](specs/erase-integrity-proof.md),
[implementation interfaces](specs/implementation-packages.md), and
[test catalogue](tests/test-catalogue.json). The complementary-pair record proof
depends on its explicit monotonicity, ECC and isolation model; portable tests
do not establish those physical assumptions.

[`evidence/r2-vectors`](evidence/r2-vectors) contains required regression inputs.
Compact public-source identities and technical verification summaries remain
under [`evidence`](evidence). Their original revisions scope those results;
the current handoff's build and regression results are in
[`firmware/build-report.json`](../../../build-report.json).

Use the [customer build/test commands](../../../../tools/firmware/README.md) for
the complete pinned source composition. See the [engineering manual](../../../../documentation/html/engineering/index.html)
for application calibration, SAVE, programming, recovery and safety behavior.
Source notices are preserved; no license is inferred for private source.
