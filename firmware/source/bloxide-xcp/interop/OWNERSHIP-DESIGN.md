# S0 serial transaction ownership (R3, before implementation)

## Source ordering and rejected design

Pinned pyXCP 016cf3e BaseTransport._request_internal takes command_lock, builds
with upstream XcpFraming, feeds CMD under policy_lock, releases that lock, calls
send, then get. SxI.send invokes the supplied serial.write. SxI._frame_listen
reads and calls the C++ receiver, whose callback queues a frame; SxI.listen
passes it to process_response. BaseTransport.process_response appends resQueue
and notifies its condition before taking policy_lock for RESPONSE. get pops
resQueue and _request_internal removes PID before Master.connect/getStatus
parse. Thus a policy record is neither a transmission receipt nor a consumption
receipt. Retaining or adding observer queues cannot establish the required
ownership. Fresh baseline artifacts reproduce all four false associations and
identity access after invalid status, with zero DOWNLOAD in that workflow.

## Owned seam and linearization

Register S0SerialTransport as a direct BaseTransport subclass: upstream
available_transports/create_transport discover it without assignments or patches.
Master still calls the unchanged BaseTransport request/error path and parses
its returned bytes. Encoding uses unchanged XcpFraming. Receive dispatch uses
the pinned get_receiver_class for LEN8/CTR8/SUM8, synchronously. No SxI listener,
frame queue, resQueue consumption, policy correlation or historical PDU lookup
is involved. This is an explicitly owned serial transport, not a claim that the
upstream asynchronous SxI transport has been fixed.

One reentrant transaction lock encloses the inherited request and get, with a
scoped receipt enclosing the Master method plus named-field checks. All ordinary
requests use the same lock; concurrent exact assertions cannot exchange receipts.
A nested command in a capture callback is refused and fences the transaction.
One immutable request/PDU/counter receipt belongs to this call only. get returns
that receipt's exact response bytes; inherited request removes PID and Master
parses that same byte string. Exact CONNECT/GET_STATUS checks run in get before
Master can parse or return an invalid handshake. Named-field assertions run in
the same receipt scope. No success receipt is retained as a global last response.

send records attempted TX, then immediately checks for waiting input after all
capture callbacks, immediately before calling serial.write. Any waiting input
fences rather than flushes/reassigns it. A full write is required. While get
waits, it alone reads serial input into one bounded frame buffer, checks length,
refuses bytes beyond the sole frame (even a partial next frame), calls upstream
checksum/dispatch synchronously, then checks remaining port input before returning.
The callback fills one local slot, never an independent queue. Unexpected PID,
malformed input, duplicate, pending input, I/O failure or timeout is terminal.
No partial frame survives to another transaction. A valid negative command
response remains a normal XcpResponseError and does not itself fence S0.

Close signals cancellation before waiting for the transaction lock; bounded
reads observe it, refuse publication and release ownership before port close.
Timeout cannot be cleared by getStatus, synch or reconnect on the same instance.
Successful DISCONNECT terminates the instance after returning its receipt.
Recovery means a new serial open/transport/receiver and a new CONNECT/GET_STATUS;
old objects and receipts cannot be reused. Raw disconnected-target ignore tests
must use a separate raw probe, not revive the terminated transport.

## Limits and disconfirming proof

The serial API cannot atomically order a remote emission with local write. Input
already visible at the pre-write check is known stale and refused. A byte-identical
delayed response arriving after the final check or after a clean write may be
indistinguishable; neither CTR equality nor an arbitrary quiet delay solves that.
Any extra bytes actually observed before return or a later write fence the
session. No hidden observer backlog exists. S0 excludes unsolicited DAQ/events,
block and pipelined traffic; those receive no fallback path to an upstream FIFO.

A hub-only prototype must first pass real PTY runs through unchanged Master and
BaseTransport: pre-write unsolicited valid CONNECT/status with invalid actual
response; stale invalid input then good actual response; external handshake gate
(no identity commands); fragmented good replies, coalesced duplicate/partial,
timeout then refused command, concurrent close and fresh reopen. Trace inherited
_request_internal's returned bytes and compare to the sole receipt and parsed
fields. Preserve original failed behavior and prototype source/hash/results.
Only after this proof may product code change. Later final tests repeat the
affected ordering requirements and full delivered regression, metadata/identity,
reference/external PTY and pinned-source/lock checks. Independent review remains
mandatory; this design does not accept T10 or actual T34 endpoint/native/HIL.

## Prototype result before product changes

Hub reproducers/prove_boundary.py with prototype_transport.py passed (exit0,
2.57s) in artifacts/prototype-proof-corrected: four CONNECT/status pre-write and
already-queued cases refused with no new wire command; identity followups were
fenced; fragmented response CTR145 was accepted and the traced inherited request
return equaled receipt.response_pdu[1:]; duplicate and partial-extra refused;
timeout and concurrent close refused further commands; fresh reopen passed.
No product Python files had changed at that point. Prototype source, test source,
result hashes and failed setup diagnosis are retained in the remediation hub.
The first failed proof was a test setup error: after refused CONNECT, Master's
DWORD_pack is uninitialized, so the followup uses the actual transport SET_MTA
request directly. Status cases still call the real identity workflow.
