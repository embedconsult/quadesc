# Profile P design delta 1: review candidate, activation blocked

This additive document is the current **candidate** refinement of
[the deferred proposal](../profile-p-proposal.md). It does not amend accepted
[portable R1–R9](../record-and-service-v1.md), APIs, records or S0. Nothing here
is implemented. The byte vectors are normative **for this candidate's design
checks only**, not an accepted wire profile. T17 remains partial. Independent
review must resolve the questions below before I6 endpoint authoring/activation.

## Evidence and open-question disposition

The bounded successor [P-OPEN disposition](../profile-p-open-v2/policy.md) now
selects a concrete application policy for independent review: single boot-initialized
logical page with local INIT_SEGMENT0 interpretation; sticky unfulfilled request,
no completion event and fully fenced diagnostic reconnect. The public standards
questions remain unproved; approval of this restricted policy is requested, not
assumed. [Reference evidence](../profile-p-open-v2/reference-evidence.md) records
complete pinned configuration/A2L inventory, actual default/single-page host
execution, fresh-file offset discrepancy, loaded-file restore and ECU/XCP divergence.
It supersedes the former vague research-next-step disposition, not F1/F2 or R1–R9.
No synchronous failure is relabeled as asynchronous evidence. The original43 wire
pairs and retained-view schema are unchanged. S0 remains immutable and F9 unknown.

## Wire cut and admission precedence

CTO/DTO<=8, byte granularity, Intel scalar order; SxI framing carries forward only
through a new matched bundle and qualified transport profile (19200 is the
current physical diagnostic baseline, not proof of a P endpoint). CONNECT remains
FF 01 00 08 08 00 01 01 in this candidate. All optional features not enumerated
remain unknown; no DAQ/STIM/PGM/seed-key/CAN claim or T38 work.

Disconnected commands except CONNECT are ignored. One request/response exchange
at a time; no pipelining. A pending storage operation after the F9 response is not
a pending wire exchange: status/readback and ordinary owner-approved RAM writes
may continue. A second wire command before the first response is safely drained
under the reviewed transport/session fence, never answered out of order.

For F9 evaluate in order: connected profile -> exact length -> mode/ID ->
calibration-owner lifecycle/disarmed policy -> selected FREEZE -> unresolved/result
capacity -> already-known service write lock/schema denial -> remaining service
lifecycle -> known rate/deadline -> reserve/stage capture/save. Known write lock
uses `Persistence::health().write_locked`; it takes precedence over the associated
`ServiceMode::ReadOnlyFault` (ordinary Stopped/Quiescing remains FE27). These are
serialized observations, not an atomic preflight API. No query establishes fresh
Q or the external authority's next permit availability before acceptance.

A definitive admission rejection creates no **accepted C** and issues no new
media command for that request. Early known-lock rejection does not even reserve
or stage a snapshot. Later validation/rate/deadline/service rejection can follow
`Broker::reserve` and `capture_owner` into existing shared scratch: that temporary
snapshot is not a newly accepted retained C. Preserve the previous summary until
`save` accepts (including exact Terminal(DurableExisting)); copy the accepted C
before FF. Fresh deadline sampling inside save remains mandatory. Reconcile any
allocated key via actual exact-key terminal/cancellation and settled retirement
before a definitive negative; Busy/Stale/Retired/KeyConflict/Unknown or a local
timeout are not proof of rejection. See [API traces](admission-correction.md).

| Request / precondition | Candidate response and effect |
|---|---|
| S0 F9, any length/mode | FE 20, unchanged unknown-command regression |
| P F9 01 00 00, Ready, freeze=1, eligible | FF only after retained Accepted and immutable C; never claims durable |
| P F9 length other than 4 | FE 21; empty packet is framing, not an F9 packet |
| P F9 mode!=01 or ID!=0000 | FE 22; modes 00,02,04,08,10 and combinations rejected |
| Known calibration-owner/disarmed-policy denial, freeze=0, or service Stopped/Quiescing | FE 27 (mode invalid), no implicit enable/no empty successful save |
| Save unresolved, result slot cannot be preserved, or rate limited | FE 10, no queue/coalescing/replacement |
| Already-known unsupported schema or `health().write_locked` (including latched recovery/quarantine denial) | FE 24, generic application admission denial; retained operation reason unchanged |
| Fresh Q failure/read issue or ConsumeErasePermit failure first discovered after Accepted | Original FF remains acceptance; exact key/C retained, then Failed(NoNewCommit) or Indeterminate as service reports; no second F9 response |
| Admission deadline definitively expired before accepted C | FE 31 only after proven preadmission termination |
| Admission/response outcome uncertain | No invented FE; fence, read/reconcile; no automatic F9 retry |
| DOWNLOAD known P read-only block | FE 23, consistent with S0 read-only-region error |
| Unmapped/cross-block UPLOAD | FE 24; MTA unchanged |
| Invalid count/extension/overflow | FE 22; MTA unchanged |

The old proposal's blanket FE32 is withdrawn **only for this candidate**:
ERR_VERIFY describes program verification, not arbitrary schema/wear/policy
rejection. FE24 is an application mapping backed by a Vector synchronous freeze
failure example, not proof it is mandatory or universally accepted for F9.
The pinned client's error matrix omits FE24 for SET_REQUEST; disabling automatic
error handling is therefore also needed to surface it directly. FE33 is not
selected: it expands the stated 1.0 subset and its client policy retries. FE27
is already in S0. No FE FF timeout; no FE25 security lock without seed/key.
Negative-code review remains part of P-OPEN-2, not self-accepted by these vectors.

Candidate PAG command bytes (reserved bytes zero; FF includes PID):

| Request | Response | Constraint |
|---|---|---|
| E9 | FF 01 01 | One segment; FREEZE supported |
| E8 00 00 00 00 | FF 00 00 00 00 10 00 00 | Segment 0 base 0x1000 in LED fixture |
| E8 00 00 01 00 | FF 00 00 00 04 00 00 00 | Length 4 in LED fixture |
| E8 01 00 00 00 | FF 01 00 00 00 00 | One page, ext0, no mapping/compression/encryption |
| E7 00 00 00 | FF 3F 00 | Page0 read/write with or without ECU access; INIT_SEGMENT=0 under the P-local boot policy |
| EA 01 00 or EA 02 00 | FF 00 00 00 | ECU or XCP page is 0 |
| EB 01 00 00 / EB 02 00 00 / EB 03 00 00 | FF | Explicit idempotent selection of sole page, never copies RAM |
| E6 01 00 / E6 00 00 | FF | Select/deselect FREEZE for subsequent requests |
| E5 00 00 | FF 00 01 or FF 00 00 | Current connection FREEZE selection |

Unknown segment -> FE28, unknown page -> FE26; invalid get/set page mode -> FE27;
invalid E6 mode, E8 selector/mapping/mode2 -> FE22. Wrong lengths/reserved -> FE21,
before semantic fields. ALL_SEGMENTS bit 80 is outside this narrow candidate.
COPY_CAL_PAGE remains FE20. Segment/PAG selection changes while an admitted save
is unresolved return FE10; page0 idempotence is not used to bypass that fence.
FREEZE initializes to 0 at each new fenced connection; it is captured as 1 at C
and cannot change the captured bytes. Other clients' saves use their own policy,
not the wire connection's selection. No automatic freeze on DISCONNECT.

## Accepted, terminal and session behavior (proposed, not sourced standards)

| Persistence observation | Originating connection GET_STATUS | Candidate result/readback behavior |
|---|---|---|
| No F9 accepted | FF 00 00 00 00 00 | No request; does not prove historical absence |
| Accepted/Pending | FF 01 00 00 00 00 | Captured C and old durable bytes are distinct |
| Durable / exact DurableExisting, settled | FF 00 00 00 00 00 | Exact canonical durable bytes + stored labels; no event |
| Failed(NoNewCommit), settled | FF 01 00 00 00 00 | Failed reason, old durable bytes, no retry; request unfulfilled |
| Indeterminate, settled or unsettled | FF 01 00 00 00 00 | Unknown outcome; retain exact C/key; read-only quarantine |
| Pre-admission rejection | Unchanged | Does not overwrite prior accepted result |
| Lost FF | Unknown to client | Fence and drain first, then resolve view/status; never repeat F9 automatically |

A lost or timed-out response first fences the wire stream: an old FF must not be
mistaken for a later command's response. Do not send status/readback on that stream
until the separately reviewed transport's idle/drain and session-fence procedure
establishes a fresh exchange boundary; use a fenced reconnect when necessary.
SYNCH alone does not provide this proof. Only an already unambiguous original
connection may poll directly. Do not auto-replay F9 during transport recovery.
Validate P build/schema/profile/A2L identity before the initial save and again
after reconnect. With other writers, pre-save scalar reads are not C: either
serialize those writers for an expected-value comparison or accept the captured
bytes/labels actually returned in the latched view. Revision equality alone is
insufficient across wrap. Persist the host's observed full key, capture and result
before initiating another save; there is no historical wire log.

Fast DurableExisting may complete before first poll: observing bit1 is not a
required intermediate wire event. `FF` from setRequest is PID-only; the pinned
Master returns empty response body `b''`, which is success, not Python truthiness.
The service and broker must record only **settled** terminal ownership before
Release; an Indeterminate diagnostic does not by itself allow release. True idle
plus drained completions is an asserted backend fact, never inferred from the
state enum or copied response. Published diagnostics may describe unsettled work
without freeing it. A later exact completion can refine uncertainty through the
existing bounded reconciliation; no extra automatic scan/write loop is added.

On same-connection settled Failed, status/read-only diagnostics continue; save
admission stays fenced for the unfulfilled request, even if failure was benign.
Indeterminate keeps write quarantine. Ordinary calibration writes still require
owner policy/maintenance permits; a persistence fault never rolls RAM back.
Disconnect/SYNCH/new CONNECT cannot cancel a save or turn uncertainty into failure.
Withhold new normal Ready until exact outstanding/control completions, broker
recording, release eligibility, old replies and transport suffixes are drained.
If true idle cannot be established, remain fenced; do not promise diagnostic
reconnect availability. After settled recovery a new diagnostic connection may
read the retained predecessor result while writes stay locked. Its status starts
at 0 (no request in this connection), with freeze=0; this reset is **not success**
for the predecessor. Session termination/bit reset follows the proposed P-local policy, pending independent approval.
No old wire response/event is replayed to the new connection. A normal namespace
advance must drain all four broker slots, copy each settled terminal summary into
its bounded diagnostic slot, and use the existing Broker.advance_epoch/service
resume handshake; the broker cannot advance with occupied slots. Broker requires
exactly +1 service epoch and +1 session generation (checked, no wrap). Preflight
exhaustion before either handshake; service resume requires Stopped, no retained
result/pending/outstanding work, no write lock and newer service/I/O epochs.
Require ownership_settled and drained controls as well. Ready requires both
handshakes to succeed; any partial handshake stays fenced. A diagnostic
connection after failure may change its connection generation while the faulted
persistence epoch stays fixed. No new saves are admitted there. Before any new
save session, broker/service generation and epoch must agree with the new route;
otherwise stay fenced. No counters/resources are reconstructed to fake this.

Wire client occupies fixed broker slot0; up to three existing internal clients
occupy slots1..3. This is a candidate composition limit, not four serial peers.
Each sees only its own retained operation and a coherent current durable view.
On wire reconnect the same fixed route may observe predecessor slot0 identity
explicitly marked with its old session_generation. This is readback, not a new
accepted response or a claim about an authenticated user. No indefinite history:
keep the last accepted result until a later explicit eligible F9 replaces it,
never on timers, disconnect or a failed read. Before replacing a settled success,
first preflight fixed adapter capacity, then consume its broker result and reserve
the next key in the same serialized adapter turn; new capture failure must
preserve the previous diagnostic bytes. This is not a new Broker API. Failure/uncertainty remains fenced.
Other saturated client results are not evicted. Epoch/sequence/generation/view
counter exhaustion fences admission instead of wrapping.

On MCU boot volatile keys/captures are gone. Expose key_valid=0/capture_valid=0;
recovered record contains stored request sequence/session and RAM epoch/revision,
**not persistence service epoch**. Never reconstruct a full live OperationKey or
globally unique boot identity from that record. Reset/reconnect in the same live
service preserves exact keys and captured bytes. A post-boot byte match proves
configuration content only, not exactly-once attribution to a lost F9. Active
Owner starts revision0 with recovered bytes; saved revision remains historical.

## Coherent readback and resource contract

This candidate replaces the earlier 320-byte readback **proposal** with a 640-byte
view because the old proposal omits captured bytes and conflates accepted key with
current durable identity. No record change. Fixture B=0x10000; production B is
unallocated until a reviewed composition assigns it. All integers LE.

| Offset | Size | Field |
|---:|---:|---|
| 0 | 8 | ASCII BLXPVW01 |
| 8 | 2 | view version 1 |
| 10 | 2 | size 640 |
| 12 | 4 | flags: bit0 key valid, bit1 capture valid, bit2 durable valid, bit3 operation ownership settled; others0 |
| 16 | 8 | view_generation, nonwrapping per live service epoch |
| 24 | 8 | current persistence service epoch (boot-local) |
| 32 | 4 | current connection generation |
| 36 | 4 | state 0=no operation,1=pending,2=durable,3=failed,4=indeterminate,5=read-only fault |
| 40 | 8 | retained key service_epoch; zero when absent |
| 48 | 4 | retained key session_generation; zero absent |
| 52 | 4 | retained operation reason only: 0 none,1 backend,2 verify,3 wear,4 version,5 ambiguous,6 timeout,7 ECC,8 policy,9 unsafe Q,10 capacity,11 rate,12 deadline |
| 56 | 8 | retained key sequence; zero absent |
| 64 | 8 | captured RAM owner epoch; zero absent |
| 72 | 4 | captured RAM revision; zero absent (revision0 is valid with flag) |
| 76 | 2 | captured schema; zero absent |
| 78 | 2 | captured length1..256; zero absent |
| 80 | 8 | durable record sequence; zero absent |
| 88 | 8 | durable source RAM epoch; zero absent |
| 96 | 4 | durable source RAM revision; zero absent |
| 100 | 2 | durable schema; zero absent |
| 102 | 2 | durable length1..256; zero absent |
| 104 | 4 | durable stored request session_generation; zero absent |
| 108 | 4 | reserved zero |
| 112 | 8 | durable stored request sequence; zero absent |
| 120 | 1 | status bit0 as observed in this view's connection at latch |
| 121 | 1 | FREEZE selection as observed at latch |
| 122 | 6 | reserved zero |
| 128 | 256 | captured canonical bytes, FF padding; all FF absent |
| 384 | 256 | durable canonical bytes, FF padding; all FF absent |

Admission rejection preserves the prior operation's key, captured labels/bytes,
state and reason byte-for-byte, including a successful reason0 and a failed
operation's original reason. FE24 conveys generic denial only; it promises no
rejected-request diagnostic detail. Never rewrite a previous success as Failed
or replace a previous failure reason for a new denied F9. No additional admission
diagnostic field/storage/schema is introduced. A refreshed view may update
view-generation/current durable/connection observations independently; existing
latched views and the retained summary are immutable. See rejected-next-save
vectors in [admission-correction.json](admission-correction.json).

Current durable data may belong to another client's more recent save; state and
retained key are this client's operation. `state=durable` alone does not say the
current record is still its result. Full retained result is held in broker slots;
readback comparison uses exact captured labels/bytes and durable metadata. If a
later record supersedes this capture, report that fact to the host, never infer
failure or claim it is still current. DurableExisting can name an earlier stored
request: compare schema, bytes, source epoch/revision, not its request key to the
new live key. Recovered historical labels never become a boot identity.

SET_MTA(B) synchronously latches one complete view for that client. Only sequential
UPLOAD n=1..7 within [B,B+640) reads it; no partial reads from a newly sampled live
view. A mid-block SET_MTA rejects FE24 without a live latch; with a live latch it
may seek within that same copy. An unrelated SET_MTA releases the latch only,
not the retained operation. SET_MTA(B) deliberately refreshes it. Exact end retires
the latch; failures leave cursor/latch unchanged. Latch invalidates on connection
fence, while the retained operation survives. Concurrent C/completion/RAM changes
cannot mix chunks. Known read-only DOWNLOAD is FE23 and changes neither view nor
MTA. No scalar A2L reads may bypass the latch protocol.

Identity is a separate immutable 128-byte block at fixture D=0x10400: magic
BLXPP001[8], descriptor-version u16=1, view-size u16=640, profile-id u32=0x50000001,
then 32-byte build hash, 32-byte descriptor/schema hash, 32-byte profile hash,
B u32, calibration-base u32, calibration-length u16, schema-id u16,
max-clients u8=4, segment-count u8=1, page-count u8=1, reserved u8=0. Fixture
hashes are synthetic non-release bytes; real bundle must bind exact image/source,
canonical descriptor/profile documents and parsed A2L addresses. No circular hash
of a file containing its own digest. Existing metadata.schema/profile IDs must
also identify P; capability addition alone cannot reinterpret an S0 identity.

[descriptor.json](descriptor.json) fixes fixture addresses, access, field layout
and limits; production address allocation is still separate.
[metadata.a2l](metadata.a2l) is a design fixture parsed unchanged by pya2ldb,
not a production exporter. BLOB supports CALIBRATION_ACCESS NO_CALIBRATION;
READ_ONLY is not a BLOB child in this parser. Explicit connection configuration
and this descriptor accompany the A2L; no IF_DATA grammar or upstream parser
extension is invented. Parser acceptance establishes syntax/fields only, not
permission enforcement, correct image identity or standards conformity. Page
metadata now includes the P-local companion policy and logical RAM segment;
independent review must approve its explicit INIT_SEGMENT interpretation.
The fixture makes no full PAG/IF_DATA or standards-conformance claim.

Preallocate four 640-byte read latches (2560), four 384-byte retained diagnostic
summaries (1536: the header128 plus captured bytes256), one publication scratch
(640), one immutable identity block (128), four 32-byte latch controls (128), and
at most128 bytes of new route/status controls: 5120 declared bytes within a
separate6144-byte P adapter ceiling. The current Broker holds Outcome but Failed
and Indeterminate do not contain captured bytes: the summaries are necessary,
not storage already present in Broker. Copy C into its summary before FF and copy
settled terminal state/key before service/broker release or namespace advance.
Keep summaries independent of disposable read latches. These are immutable
observations, never a second mutation/storage owner. Do not add an uncounted
fifth history queue. Concrete types/alignment/stack
must be measured later. This is in addition to portable <=8192, not within its
existing 7960 static tally; whole-image budget is unqualified. Profile/descriptor
JSON is host metadata, not runtime heap. At most92 uploads for640 bytes and19 for
128; status polling fixture max100 at10ms, then unknown with no F9 retry. Target
read/deadline timing and flash interference require T16. No claimed timing result.

## Future I6 ownership and independent acceptance

After separate design approval, bloxide-xcp owns F9/PAG dispatch, response ordering,
status latch, fixed read latches, metadata export and unchanged-client harness.
bloxide-persistence keeps sole save/retained/storage ownership; its optional
adapter captures via T07 public Owner during C. Composition owns addresses,
profile/build/schema bundle, maintenance policy and one-time resource moves.
No arithmetic/codec actor, alias, raw pointer or mutable-state backdoor.
Synchronous actions precede pure guards; entry/exit infallible; async backend
owned commands/completions stay outside actions. Initialize once and retain across
Reset. Use reviewed code generation only in a later authorized integration.

Independent tests must use the actual reviewed endpoint plus actual portable
service at exact pins: C21 raw/decoded F9, all tables, capture followed by RAM
mutation, DurableExisting, previous vs current durable; C22 pre/post-marker
failure, unsettled Indeterminate, lost FF, late completion, reconnect/read-only
fault, exhausted counters, true drain and no retransmit; C23 unchanged pya2ldb
parse, identity mismatch refusal before F9, multi-chunk/multi-client view coherence,
read-only/seek/overflow and storage accounting. Keep S0 F9 FE20 and its seven
commands byte-for-byte. Encoding probes here do not satisfy C21-C23. No endpoint
stub is evidence of acceptance. Do not start T11/T18/T38 or edit active T04/T28.

I5 still requires reviewed T06 + actual T36 runtime/allocator qualification; no
retry of their service blocks. The user deferred T34 independent Git acquisition for this demo; later integration
may use the reviewed local source arrangement with exact provenance. No T34
packaging/auth retry is part of this job. I7/T16 still owns exact regions/G/E/ECC,
monotone erase/program/isolation/Q stability, RAM execution/timing, production
externally durable wear authority and real idle+drain. This design proves none
of those physical facts. No native ARM64/MCU/HIL result, bridge/device publication,
MAIN/NONMAIN/security/fuse/CAN/motor/supply operation is authorized here.
