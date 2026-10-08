# T17 approved portable persistence contract r2

This contract was approved at design commit 82aa2c5bfbfbeaca67921d41ef087d6e441fb064.
Portable I1–I4 implementation is in this repository; actual correction API details
are in portable-correction-r1.md. It is not an accepted extension to S0. It selects
two disjoint erase units, one record per unit, for payloads of 1..256 bytes. No
journal compaction, migration, arbitrary-address write, flash PGM service or
runtime dependency is in this cut. All logical integers below are unsigned little-endian.
F1 correction: [proof and fault model](erase-integrity-proof.md) is normative for
R1/R2/R3/R7. The filename is retained as a document link, not a format-v1 alias.

## R1 — Inputs and backend guarantees

Construction validates these immutable inputs before external admission. Geometry
and codec registration remain fixed for the provisioning lifetime; changing them
requires separately reviewed migration/reprovisioning, never another boot guess:

| Input | Required contract |
|---|---|
| `E`, `G` | Erase-unit bytes and program granule bytes. G is power of two, 1..256; E is a multiple of G, 512..65536, and E >= M+G. M = round_up(640,G). Other geometry returns UnsupportedGeometry. |
| A, B | Disjoint E-byte regions, each E-aligned, exclusively owned; outside all firmware, boot, security, NONMAIN and other data. Use Slot A/B identifiers in portable commands, never physical addresses. |
| Erased state/transitions | FF bytes. Interrupted erase only raises arbitrary addressed-unit zero bits to one, including stored CRC/marker; no order assumption. Interrupted program only clears a subset of requested bits in the addressed G-byte granule. No overprogram or nonmonotone transitions. Each granule is programmed at most once between erases, including commit. |
| Isolation | An interrupted erase/program can damage only the addressed erase unit/granule respectively; the other slot and already finished granules remain stable. All finished bits remain stable outside the addressed operation; interrupted erase may affect any subset of zero bits within its unit, not merely an ordered prefix. |
| Durable completion | A successful completion means operation finished, voltage/timing requirements met and persistent media state settled. Read returns coherent uncached bytes or explicit read/ECC error; no fabricated FF for unreadable data. |
| Restart | After power cycle the backend establishes that no old write remains active before reads. On logical Reset/timeout it must prove quiescence; resetting a software flag is insufficient. |
| Readback | Backend can read the full slot and distinguish uncorrectable error. Corrections are reported; conservatively quarantine a slot with a corrected error too. |
| Limits | Bounded maximum erase/program/read/quiesce times and watchdog/interrupt policy are required inputs; no unbounded backend polling in an HSM action. |
| Wear authority | Persistently accounted, non-reissued erase permits, per-slot remaining budget, stable quarantine information; see R8. |
| Erase eligibility | Full healthy pre-erase scan must satisfy Q in R3; administrative recovery authorization never waives it. No intervening writer or spontaneous disturbance in the modeled interval. |
| Schema | Nonzero schema ID (u16), exact length <=256, bounded deterministic encode/decode/validate/default callbacks and application policy. Unknown schemas are never guessed. |

These are required guarantees, NOT statements about AM13. T16 must supply exact
MAIN regions, chip revision, G/E/ECC coupling, disturbance scope, allowed repeated
programming, RAM execution/call graph, cache/interrupt access, endurance and timing.
If physical disturbance extends beyond a program granule, G must encompass that
unit and remain within this profile, otherwise reject this backend/profile.
If two independent erase units are unavailable, do not emulate them in one page.
No hardware deployment can proceed without these proofs. The monotone bit model,
including ECC visibility/coupling and voltage-collapse behavior, is UNQUALIFIED
on AM13. An all-cut theorem under these explicit inputs is not a hardware claim.
Arbitrary nonmonotone replacement is distinct residual corruption, not an admitted
modeled interruption that can be silently excluded as a CRC collision.

## R2 — Exact record layout

There are two coordinate spaces. Logical body L is exactly320 bytes (64-byte
header plus256-byte payload area). Physical slot S has E bytes. Encode EVERY
logical byte, including CRC and logical padding: `S[2*i]=L[i]`,
`S[2*i+1]=L[i] XOR FF`, for i=0..319. Complement test is bitwise XOR=FF for each
adjacent byte pair. Each bit pair is01 or10, never00 or11. No raw field can bypass
this check. Decode L only after all320 pairs pass. Physical code body is640 bytes;
M=round_up(640,G), programmed span M+G <=1024, separate one-shot commit granule.

| Logical offset | Logical bytes | Meaning / validity |
|---|---:|---|
| 0 | 8 | Magic hex `42 4c 58 50 45 52 53 32` (BLXPERS2) |
| 8 | 2 | Record format = 2 |
| 10 | 2 | Application schema ID, nonzero |
| 12 | 4 | Payload length 1..256; must equal registered schema length |
| 16 | 8 | Persistent record sequence 1..2^64-1 |
| 24 | 8 | Captured RAM owner's service_epoch |
| 32 | 4 | Captured RAM owner's ActiveRevision.get() |
| 36 | 4 | Persistence request session_generation |
| 40 | 8 | Persistence request sequence (operation identity, not record sequence) |
| 48 | 12 | Reserved, exactly zero |
| 60 | 4 | CRC-32C, encoded just like every other byte |
| 64 | length | Canonical schema payload bytes; no struct padding/pointers |
| 64+length | 320-(64+length) | Logical padding exactly FF (physical pairs FF,00) |

| Physical offset | Bytes | Meaning / validity |
|---|---:|---|
| 0 | 640 | Encoded logical L; logical range [a,b) occupies physical [2a,2b) |
| 640 | M-640 | Alignment padding exactly FF, outside logical CRC |
| M | G | Commit marker: every byte exactly00 |
| M+G | E-(M+G) | Unused tail exactlyFF |

For G1,2,4,8,16,32,64,128, M=640; G256 has M=768. Minimum
legal E is `round_up(max(512,M+G),G)`: 641,642,644,648,656,672,
704,768,1024 respectively. E512 is now unsupported for every G; that is a
portable layout cost, not a claim that a particular device offers larger sectors.
Total flash reservation is2E. All bounds are checked before indexing/commands.

CRC-32C/Castagnoli parameters remain polynomial0x1EDC6F41, reflected0x82F63B78,
init/xoroutFFFFFFFF, refin/refout true; ASCII123456789 -> E3069283. Feed decoded
logical L[0,60), then L[64,320), ascending order. Exclude L[60,64); physical
alignment/marker/tail are outside CRC and checked exactly. CRC no longer depends
on G. It remains a corruption detector, not the erase-integrity mechanism.
The complementary encoding covers stored CRC too: simultaneous data/CRC erase
changes cannot map between distinct accepted bodies. No CRC-collision exception
is needed for the modeled interruption theorem.

This replaces the rejected, never-implemented raw BLXPERS1 proposal. Raw v1 is
not a legacy alias, not parsed as an alternate layout and not migrated on boot.
The fixed encoded BLXPERS2 envelope is the only candidate format here.

LED example schema ID 1: length 4; period u16 milliseconds then duty u16 permille;
validate 100..10000 and 0..1000, defaults (1000,500). This schema belongs to the
consumer, not portable library constants. Unrelated thermostat fixture schema ID
2: length 8; signed little-endian i32 setpoint_mC 5000..35000, u32 hysteresis_mC
100..5000, additionally setpoint-hysteresis >= 0; defaults (20000,1000). Portable
core only sees registered schema codec and bytes. Neither fixture is firmware.

## R3 — Validity and recovery precedence

Scan both full slots into bounded scratch storage after qualified idle; read/ECC
error is Unreadable (including an error accompanying FF bytes). A healthy all-FF
slot is Empty. Inexact commit marker is Uncommitted. With exact-zero marker, any
noncomplementary body pair is Corrupt; do not inspect untrusted version/sequence
first. Decode the fixed320-byte body, then verify magic, nonzero schema/sequence,
length1..256, reserved zeros, logical padding, physical alignment/tail and CRC.
Any failure is Corrupt. This defines envelope validity independently of supported
format/schema. Only then may unsupported format/schema classify Unsupported.
For supported format2 and registered schema, require its exact length and full
semantic decode/validation to classify Valid. Format0 is unsupported if its
common envelope is valid; schema0 or sequence0 is always malformed. Never invoke
an unknown decoder or substitute that schema's defaults.

BLXPERS2 declares this exact fixed logical/physical common envelope and CRC domain
across future format values. A future incompatible encoding/geometry needs new
magic plus separately reviewed migration, never a heuristic alternate decoder.
Every accepted field, including version/schema and stored CRC, lies within the
complementary body. Partial erase cannot manufacture an Unsupported record or a
duplicate-sequence ambiguity from a valid predecessor; see R7.

| Slot classifications | Boot result |
|---|---|
| Any Unsupported | Explicit Defaults(UnsupportedVersion), reads/diagnostics only; no erase/save until separately reviewed migration/reprovisioning. Even a lower-sequence unknown record triggers this policy. |
| Two Valid, distinct sequence | Choose greater numeric sequence; no modular comparison. |
| Two Valid, equal sequence, identical bytes [0,M+G) | Choose A deterministically, report Duplicate; both represent the same configuration. |
| Two Valid, equal sequence, different bytes | Defaults(Ambiguous), lock writes; never guess newer by address or CRC. |
| One Valid | Load it; report other slot's classification. Backend recovery/quarantine must authorize reuse of the other slot and Q below must pass before erase. |
| Neither Valid | Defaults(reason bitset Empty/Uncommitted/Corrupt/Unreadable); keep explicit diagnosis. Only two Empty, healthy slots are automatically writable. Otherwise require backend recovery authorization AND Q before erasing either. With no selected Valid record, an authorized defaults-start uses sequence1 and the same defaults/new oracle; never trust corrupt sequence bytes. |

Before EVERY erase, perform a complete healthy read of the inactive target in
QualifyErase. Qualified stable reads and exclusive ownership hold until command
completion/reconciliation. The structural erase-eligibility predicate Q accepts:

- Q1: any marker bit is1; OR
- Q2: any bit position in a body byte pair has both bits1 (`d & complement !=0`);
  OR
- Q3: every body pair is complementary, marker is zero, physical alignment/tail
  are FF, and normal classification is Corrupt or a subordinate Valid record
  (lower sequence than selected current, or exact duplicate of it).

Unsupported/ambiguous recovery, read/ECC error, selected-slot discrepancy or
unreconciled backend quarantine denies writes regardless of Q. A Valid target
with no selected current is a recovery discrepancy, not an erasable default.
If none of Q1-Q3 holds, retain Failed(NoNewCommit, UnsafeErasePrestate), lock writes,
consume no erase permit and require separately reviewed reprovisioning. No partial
program, invalidation-marker trick or administrative authorization may bypass Q.
QualifyErase can stream: retain any-marker-one, any-pair11, all-pairs-complementary,
exact padding/tail flags and bounded decoded body. G1 reads may bridge pairs using
one carry byte. Fixed memory only. See the proof's00-pair and damaged-tail negative
controls; unconditional erasure of arbitrary invalid records is unsafe.

Defaults must themselves pass the registered validator or startup fails closed
with NoConfiguration; never begin Ready with invalid values. No automatic format
migration or destructive factory reset in this API. Hardware reprovisioning is
outside ordinary Save/Reset.

Record sequence starts at 1 on two healthy blank slots. New sequence is current
valid maximum+1. At MAX, reads/boot work but new saves reject SequenceExhausted;
no wrap or erase-to-zero. Exact already-durable snapshot may return DurableExisting
without writes even at MAX. Equal payload alone is insufficient for that fast path:
schema, payload, captured RAM epoch and revision must all match validated durable
record metadata. A sequence gap is legal. Corrupt sequence bytes are not trusted.

Boot installation is a separate composition step: validated recovered values go
to Owner::new once, before Ready/output initialization. Recovered record metadata
remains separately visible. Owner starts its normal revision 0; do not overwrite
its private revision with the saved number. Logical Stop/Start/Reset preserves RAM
values and saved metadata; only MCU boot reloads storage. Storage failure during a
running session NEVER rolls back or otherwise changes active RAM configuration.

## R4 — Snapshot and request contract (proposed API, not existing T07 API)

T07 is consumed only at exact calibration commit
`c0a693a8811d920d05850cfc2ebcfd7956a2741b`. Its actual public methods are
Owner::active(), active_revision(), service_epoch(), session_generation(), mode().
The adapter runs inside the sole RAM owner's synchronous turn, holds an immutable
owner borrow across these reads, validates/encodes a complete copy, then hands the
owned fixed-size snapshot to persistence. No per-field interleaving or await.
A request reaching another actor must return to this owner for capture; it cannot
read mutable actor fields through shared pointers. No changes to T07 are needed.

Capture point `C`: after validating current owner mode/policy, schema and optional
expected (epoch,revision), reserving the single save slot and completion capacity,
and freshly checking admission deadline, copy complete values and named revision.
All steps are bounded and synchronous. Failed reservation releases fixed slots
without heap calls. Only then return Accepted. Snapshot is private validated data
containing schema, len, [u8;256], source epoch/revision and request identity. Later
RAM Apply operations continue through T07 and cannot modify this copy. Storage
writes are not Apply and never reserve a GPIO output slot.

ActiveRevision wraps u32 in accepted T07. It is a label, not a durable sequence or
global unique identity. Expected-revision matching is scoped to the synchronous
capture turn; clients cannot infer equality of bytes from revision alone across
long gaps/wrap/boot. Durable readback includes canonical bytes and record sequence.
Boot identities are unavailable; saved source epoch/revision are historical labels.

Use canonical OperationKey widths {service_epoch:u64, session_generation:u32,
sequence:u64}, imported in the adapter, with a distinct persistence service
epoch/namespace. Storage record sequence is separate. RAM source epoch is captured
from the RAM owner and may differ from persistence key epoch. A live broker
allocates increasing sequences across up to four fixed client slots; do not create
four mutation owners or four simultaneous flash writers. No sequence/generation/
epoch wraps: fence/drain before generation change; exhaustion locks admission.

| Proposed service operation | Meaning |
|---|---|
| Save(key, captured snapshot, admission_deadline) | Classify key first. If capacity/policy permits, retain Accepted/Pending and own snapshot. Deadline sampled freshly at C; expiration after acceptance is not cancellation. |
| Resolve(key) | Same-key Pending or exact retained terminal; Retired/Stale includes current durable view, never implies never-stored. |
| ResolveOrCancel(key) | Before acceptance create retained Cancelled tombstone; accepted request returns Pending (no cancellation of admitted flash); terminal replays. |
| Release(key) | Only exact terminal after broker records it; advance watermark before clearing slot. Duplicate release for fenced key is idempotent. Cannot release Pending. |
| ReadDurable | Return coherent {record sequence, source epoch/revision, schema, length, bytes, health}, or explicit defaults/no-record. Never return active RAM as durable bytes. |
| Quiesce / Resume | Fence new requests, drain current backend work and reconcile terminal before acknowledging. Resume requires qualified backend Ready and fresh epoch handshake. |

Exactly one admitted save and one terminal result slot. Lookup order: retained or
pending matching key (different same-key snapshot returns KeyConflict without
changing original), then stale epoch, then retired watermark, then Busy if occupied,
then admission validation. Definitive validation/deadline rejection occupies the
terminal slot until release. Busy/Retired/Stale are observational, unretained and
do not consume sequence. No silent queue behind a save. Broker records terminal
in the originating client's one fixed result slot before release; same client
cannot start another request until consuming it. Four saturated client slots
cause Busy, not eviction. Quiescing is also an admission latch across intermediate states, including Retained.
Release cannot reopen a stopped, quiescing or quarantined service. No timer expires retained results; disconnected clients
are reconciled by the broker before their session can be fenced/reused.

One global save differs from T07's one global RAM mutation: RAM edits may proceed
after C while flash is pending. Their separate brokers use distinct service
namespaces and independent retained slots; no legacy aliases or widened T07 API.
Motor consumer policy accepts capture only while disarmed and must hold a platform
maintenance permit preventing arming during flash if T16's timing requires it;
that integration permit is not fabricated by this portable service.

## R5 — Owned backend commands and completion transitions

One constructor-created flash handle is moved into the backend owner once. The
service emits an owned command to a preallocated capacity-one command slot; backend
retains one completion until acknowledged. Each carries {io_epoch:u64,
command_sequence:u64, slot, kind, offset, len}. Program carries [u8;256] plus G;
Read carries no borrow and completion owns [u8;256] plus len. Read full slots in
<=256-byte chunks. I/O sequence and epoch use checked increments and never wrap; exhaustion locks
backend admission after the current command reconciles. Exact command correlation is required; duplicate completed
commands are never reissued to hardware. Correlation mismatch fences the service
and reconciles the actually outstanding command; it must not discard it.

| State | Successful action/completion -> next state |
|---|---|
| Recovering | Backend Ready, scan/classify -> Idle or ReadOnlyFault |
| Idle | C owns snapshot/result capacity -> Accepted/QualifyErase |
| QualifyErase | Full healthy target scan, unchanged recovery selection and Q pass, backend permits recovery -> AcquireWear; else retain failure/read-only, no erase |
| AcquireWear | Durable consume-once permit for inactive slot -> Erasing |
| Erasing | Erase inactive E-byte unit exactly once -> VerifyErased |
| VerifyErased | Chunk reads verify entire unit FF -> ProgramBody |
| ProgramBody | Encode complete logical header/payload/CRC, then all complementary pairs in fixed image; program G bytes at offsets 0,G,..,M-G, one command at a time -> VerifyBody |
| VerifyBody | Read body, erased marker/tail, compare exact planned bytes and validate complement code/CRC/schema -> ProgramCommit |
| ProgramCommit | Program all-zero G-byte marker once -> VerifyCommitted |
| VerifyCommitted | Read entire slot; run boot validity, compare exact snapshot and sequence -> Retained(Durable) |
| Retained | Read/resolve/replay allowed; matching Release -> Idle only when admission is enabled; otherwise Stopped after quiescence or ReadOnlyFault |
| Quiescing | Stop admission; finish accepted workflow or reconcile uncertainty; retain result, drain/ack backend and broker -> Stopped |
| Stopped | Preserve everything; coordinated Resume/Ready/epoch fencing -> Idle |
| ReadOnlyFault | Read valid records/default status only; no automatic retry/reacquisition |

Before first record choose A; thereafter choose the other slot. Never erase the
selected current slot. Never erase the previous record as cleanup after committing
the new one. Each new save erases exactly one inactive slot, even when it looks
blank (uniform wear/accounting). One program per body granule then one marker;
no in-place field updates. `DurableExisting` bypasses all backend writes.

CRC/complement encoding, Q and per-chunk validation are synchronous bounded library work;
entry/exit actions infallible, guards pure, actions before guards. Async waits exist
only in the backend service; no HSM action awaits. Platform pump provides progress
on owned command/completion slots without lost wakes, including retained slot
saturation. Proposed mailbox-free resource wiring must await reviewed T06 syntax;
no fictitious package lock or generated Rust is included here.

## R6 — Failures, reset and interrupted traces

Before marker issue, a definite backend error followed by proved quiescence yields
Failed(NoNewCommit), retaining old durable view. At/after marker issue, readback
may find the exact new valid record despite a lost completion: return Durable only
after qualified quiescence and verification. Invalid target -> Failed(NoNewCommit);
unreadable media or unproved quiescence -> Indeterminate, lock writes. Never call
an uncertain outcome Rejected/Cancelled. Durable result notification loss is not
storage failure. Failure before acquisition/erase admission consumes no erase;
any admitted erase consumes its wear token, even if no byte changes.

Timeout: request bounded quiescence once, retaining the outstanding command;
no retry of erase/program. A Quiesce acknowledgement is withheld until hardware
is proved idle and all completions/results are reconciled. If quiescence cannot be proved, remain quarantined with
Indeterminate and no more writes. Readback after quiescence may inspect a
quarantined region but cannot clear quarantine. Backend recovery authorization
must explicitly clear it; a service Reset cannot. ECC/verify/backend failures
quarantine the affected slot and block further two-slot saves. Recovery may still
load the other valid slot. Stale completion cannot advance state. Actor Reset
preserves resources, snapshot, outcome, fences and quarantine; epoch advances only
after work/result reconciliation and queue drain. MCU power loss loses volatile
results/keys, never promises exactly-once across boots; boot scan is authoritative.

Bounded example (G=8, E=1024, schema length=4): M=640, 80 body programs,
one marker program, one erase, sixteen <=256-byte reads (4 qualify target,
4 erased-check, 4 body-check, 4 committed-check). Boot scans are eight reads total.
AcquireWear adds one request: normal save =99 commands. Generally
`3 + M/G + 4*ceil(E/256)` commands, maximum1667 at G1/E65536. Boot <=512 reads.
Finite reconciliation adds at most one quiesce plus one full target scan; no
command retry, no refresh of finite receipt/sequence budgets. Backend authority
internal work has its own finite qualified deadline/budget, not hidden loop retries.

Trace: A=(seq7,rev10,2000/250), B=(seq6,rev9,1000/500), C captures rev11=
3000/750 for seq8; RAM then becomes rev12=4000/500. Cut during B erase/body -> A.
Cut during marker -> A unless B marker already exact zero, then B seq8/rev11.
After full marker but before response -> B. Completion -> Durable(seq8,rev11),
active remains rev12. Boot installs B values with Owner revision0 and historical
saved source rev11. No old queued request survives MCU reboot. The exact F1 erase
mask applied to B's encoded data rails produces11 pairs and leaves A selected.

## R7 — Safety argument and its limits

[erase-integrity-proof.md](erase-integrity-proof.md) gives the normative theorem,
prestate guard argument and induction across successive interruptions. Every
complementary body has equal bit weight; no two distinct bodies are comparable
under erase's componentwise0->1 order. Q prevents an independently invalid prestate
from acquiring new recovery authority during erase. Old valid subordinate records
can remain unchanged; they cannot be promoted, become Unsupported or cause a new
duplicate ambiguity. After verified erase, partial body programming remains11 or
the intended pair; commit is last and isolated. Thus every admitted cut under R1
recovers selected old or exact captured new (defaults/new for blank/default start).
No CRC-collision exception discards any modeled erase/program interruption.

This is a conditional mathematical statement. Finite prefix/subset and directed
searches exercise the specification but are not product or physical proof. Arbitrary
nonmonotone accepted-codeword replacement is undetectable in general by any finite
local encoding. Detected corruption follows R3/R6 with Q still mandatory before
reuse; unreported physical replacements, isolation/ECC/model violations remain
explicit unqualified physical risks. No AM13 monotonicity/geometry claim follows.

## R8 — Wear, finite budget and quarantine

Set a qualified budget `W_A`, `W_B` <= derated backend minimum guaranteed erase
endurance. Never infer endurance from chip family or successful tests. Every
admitted erase, failed/interrupted included, spends one permit BEFORE command
publication. Permits are unique, tied to physical slot identity and provisioning
generation, and can never be reissued after crash. Preallocated live counters
alone and sequence numbers cannot account for an erase interrupted before record
commit. This is a necessary backend input, not hidden safety in the dual records.

Selected portable contract: backend owns durable monotonic budget authority and
supplies ConsumeErasePermit(slot, operation) with an idempotent durable receipt.
It must deny further writes after reboot if that authority cannot be reconciled.
The core never claims to implement that authority using these two data slots.
A conservative externally persisted provisioning allowance is acceptable for a
host test/backend; production T16 must separately design and review its authority
or remain read-only after loss of its qualified allowance. No invented AM13 counter.

The bounded simulator authority may use two counters and a fixed `W_A+W_B`
receipt table, indexed by provisioning identity/physical slot/issuance identity,
not volatile operation key alone. For the workload below this is exactly 200
receipt entries; replaying a receipt never grants a second erase. It lives outside
the simulated MCU reset domain. If the authority's own persistence is lost or
unreconciled, deny new writes; no unbounded renewal journal or implicit replenishment.
Host-process crashes require separately qualified external storage too. This is
the existing wear seam, not a boot-selection authority and not a provided driver.

Bounded simulator workload sets W_A=W_B=100, minimum interval=1 simulated second
between new write admissions (including failures), and four clients. At most 200
erases per provisioning lifetime, no refresh on Reset/reconnect/reboot; alternating
successful saves yield exactly 200, then WearExhausted. Failed wear receipts may
waste a token but never permit an unaccounted erase. Receipt storage's own wear is
the authority's responsibility and part of T16 review. Rate limiting is extra load
control, not proof of lifetime accounting. New-save validation defaults to rejecting
inside the interval; never queues or merges snapshots. Document target W and time
source at implementation qualification, not by reusing this synthetic value.

## R9 — Allocation/resource envelope

No alloc/alloc_zeroed/realloc/dealloc after freeze, even failure/drop/reset. Fixed
storage: two physical images of at most 1024 bytes each (planned body+marker and read scratch),
one decoded logical scratch body of 320 bytes,
one snapshot <=256 bytes, one owned program/read-completion buffer <=256 each,
one global result, four broker result slots, two health counters and fixed trace
ring (32 records x 32 bytes). Stream full slot scans; E does not size RAM arrays.
Implementation must publish actual sizeof/alignment totals (budget <=8192 bytes
for core+adapter+slots, excluding platform driver and runtime) and linker results.
No Box/Vec/String/Arc or lazy tasks in core; process-lifetime runtime resources are
initialized/moved once. Completion failure and cleanup only clear fixed slots.
The real runtime, first poll, Stop/Reset/fault/drop and saturation guard test is
still T36/integration evidence; a no_std compile is insufficient.
