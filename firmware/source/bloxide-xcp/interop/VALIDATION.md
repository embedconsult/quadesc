# T10 R3 response ownership validation

The starting point is 799d10d2dd210535e1670d3a21c502b3082badef. F1 parsed
metadata/identity, F2 connection profiles and F3 DOWNLOAD precedence remain
preserved. F4-R3 is the only corrected product boundary. The preimplementation
[ownership design](OWNERSHIP-DESIGN.md) records upstream ordering, the hypothesis,
disconfirming tests, prototype proof and limits.

S0SerialTransport is an owned BaseTransport subclass discovered by the unmodified
pyXCP factory. It keeps upstream request framing, command error handling, SxI
checksum receiver and Master parsing. Receive runs synchronously in the request
owner: there is no SxI listener or independently correlated observer queue. get
validates and returns the same immutable raw PDU that inherited _request_internal
passes (minus PID) to Master. Exact assertions own a receipt only within their
Master call/named-field validation scope. CONNECT must be FF01000808000101 and
GET_STATUS FF0000000000. Raw validation occurs before Master parsing, including
reserved bits that named fields omit. No second CONNECT or last-response guess
is used. The old correlator API and its synthetic callback unit test are removed.

Known input after the pre-write capture callback and immediately before actual
serial.write fences the transport. A complete incoming read batch may contain
only one response; extra complete or partial bytes, malformed frames, unsolicited
non-response traffic and queued bytes before consumption/next write all fence.
The request and response CTRs remain independent. Timeout, partial write, I/O
failure and close are terminal; DISCONNECT completes once and terminates that
instance. Reopen uses a new ClientSession, serial open and receiver. Close can
cancel a blocked receive before waiting for the shared transaction lock. A raw
separate probe tests the target's disconnected ignore behavior.

No serial API proves remote emission timing in the gap between the final input
check and write. Byte-identical delayed replies arriving after that boundary can
be indistinguishable. This implementation refuses locally observed ambiguity;
it does not claim exactly-once execution, natural race incidence, hardware timing
or response-counter identity. The upstream asynchronous SxI transport is unchanged
and is not claimed repaired. This explicitly owned S0 transport is the selected
client configuration for subsequent runs.

The delivered tests use real independent PTY framing for pre-write/stale input,
external identity gating, actual inherited get/request consumption, fragmentation,
coalesced duplicates/partial input, late traffic, nested/concurrent commands,
timeout/disconnect/close and fresh reopen. Existing metadata, identity, negative
command, reference and explicit external19200 tests remain required. DOWNLOAD
syntax FE21 precedes semantic FE22 unsupported-width checks; simulator product
code and the preserved 93-case independent F3 matrix are unchanged.

## Verification scope

The original transport correction was tested with independent PTY framing,
metadata/identity gates, malformed and extra response bytes, close/disconnect
behavior, and nested/concurrent command ownership. Those fixtures remain in
source. They establish host protocol behavior rather than electrical timing or
physical device acceptance. The source revisions and public dependency identities
are retained in `integration-lock.json`; current handoff regression results are
recorded in `firmware/build-report.json`.
