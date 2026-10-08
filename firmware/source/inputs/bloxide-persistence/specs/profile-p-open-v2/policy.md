# Profile P open-question disposition: bounded application policy candidate

This is the proposed minimum additive profile for the authorized UART LED save /
reboot-restore demo. It completes the **design prerequisite for review**, not
standards qualification, I6 release, endpoint implementation or task acceptance.
Read it together with [the wire/view contract](../profile-p-design-v1/design.md)
and the approved [admission correction](../profile-p-design-v1/admission-correction.md).
R1–R9, runtime Rust, four client summaries, all 43 wire pairs and LED schema remain
unchanged. The source/evidence limitations below are intentional review inputs.

## P-OPEN-1: select an explicit boot-initialized, single-page application model

**Decision proposed for independent approval:** P has exactly one logical segment
(number0), one mutable calibration page (number0), with ECU and XCP both accessing
the same calibration owner. `INIT_SEGMENT=0` is a **P-local boot-initialization
annotation** identifying that same logical region. It does not promise a separate
readable immutable initialization image. This self-reference is not offered as a
universal interpretation of the ASAM field. A generic tool requiring an independent
initialization image is outside this demo profile and must reject it before writes.
The dedicated pinned client explicitly consumes this policy and its matched identity.
There is no client-driven page initialization or copy operation in this profile.

At MCU boot only, the persistence owner establishes qualified backend idle, scans
and validates R3, chooses the valid record or explicit validated defaults, and
constructs the RAM Owner once before Ready/output initialization. Recovery error
classification/write lock remains visible; absence of a valid record never silently
claims restoration. The initialized bytes then become the ordinary writable page.
RAM Apply changes the page; SET_CAL_PAGE, FREEZE, CONNECT, logical Reset/Stop/Start,
and a storage failure never reload or roll back it. Reading segment0 later reads
current RAM, **not the boot initialization bytes or durable storage**. Durable
readback remains the separately latched view. The two physical slots are never
exposed as XCP pages, regions or writable addresses.

FREEZE is explicit per connection: start0, E6 01 00 selects1, E6 00 00 deselects0.
It selects eligibility, not immutability. F9 with freeze0 is FE27 (never vacuous
success). At accepted capture C, the entire registered schema and labels are copied
in the owner's synchronous turn. Later RAM edits cannot alter C. Page/FREEZE
mutation while a save is unresolved is FE10. No automatic save on disconnect.

This requires **no extra page, segment, COPY_CAL_PAGE, PGM, event, backend API or
new storage block**. CAL/PAG resource is already bit0 in CONNECT; the P identity
and companion descriptor enumerate these capabilities, rather than changing S0's
meaning. Exact existing P vectors remain:

| Command and full response | Widths and semantics |
|---|---|
| E9 → FF 01 01 | 1-byte request, 3-byte response: one segment, FREEZE bit0 |
| E8 00 00 00 00 → FF 00 00 00 00 10 00 00 | 5/8 bytes; u32 LE base 0x1000 |
| E8 00 00 01 00 → FF 00 00 00 04 00 00 00 | 5/8; u32 LE length4 |
| E8 01 00 00 00 → FF 01 00 00 00 00 | 5/6; u8 page count1, extension0, mappings0, compression0, encryption0 |
| E7 00 00 00 → FF 3F 00 | 4/3; u8 properties3F, INIT_SEGMENT0 under this policy |
| EA 01 00 or EA 02 00 → FF 00 00 00 | 3/4; u8 ECU/XCP page0 |
| EB 01 00 00, EB 02 00 00, EB 03 00 00 → FF | 4/1; idempotent selection; no copy/reload |
| E6 00 00 or E6 01 00 → FF | 3/1; select FREEZE0/1 |
| E5 00 00 → FF 00 00 or FF 00 01 | 3/3; current connection's FREEZE |
| F9 01 00 00 → FF | 4/1; mode u8=1; ID u16=0; accepted C only |
| FD → FF ss 00 00 00 00 | 1/6; ss u8=00/01 below, protection/reserved/config-ID zero |

All exact length/reserved checks precede semantics (FE21). Unknown segment FE28,
unknown page FE26, invalid page-access mode FE27, unsupported E6 mode or E8
selector/mapping/mode FE22. Mode80 ALL_SEGMENTS excluded. EB mode0 and EA mode0/3
are invalid. COPY_CAL_PAGE and all S0 F9 remain FE20. Semantic selection mutations
while unresolved return FE10 after their syntax/field validation; getters remain
available on an unambiguous connection. All response reserved bytes zero; the
reference's stale reserved bytes are deliberately not copied.

`policy.json` binds these widths, the unchanged descriptor and exact capabilities.
The A2L adds a standard MEMORY_SEGMENT for logical RAM plus period/duty VALUEs;
P page/command/initialization policy remains explicit companion metadata. No
invented IF_DATA, vendor extension, auto-discovery or CANape qualification. S0
A2L/export is unchanged. Identity hashes must include this policy and new A2L in
an acyclic release manifest; the existing synthetic identity is still unusable.

## P-OPEN-2: select no-event, fail-closed asynchronous result policy

**Decision proposed for independent approval:** retain the original request's bit0
until confirmed fulfillment or a fully fenced new connection. Failure is expressed
by the existing retained diagnostic result, never by reusing the F9 response.
A synchronous denial before acceptance and a failure after FF are distinct cases.
The following is application policy, not a claim about every XCP implementation:

| Observation | Original connection bit0 | Allowed next behavior |
|---|---:|---|
| Receipt / staged capture only | unchanged | No accepted C or FF yet |
| Definitive admission rejection | unchanged | One FE per corrected precedence; preserve old result/key/C/reason |
| Accepted, immutable retained C | 1 | Exactly one FF; ordinary permitted RAM writes/readback may continue |
| Pending | 1 | Poll status/view; no second save, no automatic retry |
| Exact Durable or DurableExisting, settled | 0 | Record exact terminal before release; compare canonical bytes and labels |
| Settled Failed(NoNewCommit) | 1 | Keep old durable bytes; original save session remains fenced |
| Indeterminate with possible late I/O | 1 | Read-only quarantine; retain custody even after diagnostic publication |
| Late exact completion proves Durable, settled | 0 | Refine same key/C; no second FF/event; record before release |
| Safe diagnostic reconnect after failure | 0 in new connection | Freeze0; predecessor failure retained; saves remain locked |
| Reboot | 0 in new connection | New boot has no live request; classify stored record and report restore/default separately |

No EV_STORE_CAL, EV_USER, EV_CMD_PENDING, failure event, forced event-driven
termination, extra status error bit, or later FE for the accepted F9 is emitted.
Host polling has a finite application deadline; exceeding it reports unresolved,
stops writes and enters transport fencing. It does not turn a pending request into
Failed, clear the bit, release a slot, cancel physical work, or trigger an F9 retry.
This policy deliberately requires this application-aware client; a generic master
that treats a sticky bit as ongoing work indefinitely cannot be used unattended.

Same-connection failure keeps reads available only while the exchange boundary is
unambiguous. Reconnect is not a cancellation mechanism. Before diagnostic reconnect
or normal Ready, reconcile exact outstanding/control completions, prove backend
idle, record all settled terminal results, retire/release all broker slots, and
drain old command replies/transport suffixes. No timeout/enum proves idle. If these
facts cannot be established, remain fenced; there is no promised reconnect deadline.
A late completion is owned by the original backend operation even if the client
has timed out. Status0 after reconnect says only that the new connection has not
accepted F9; it never proves the predecessor durable.

Retained failure remains available on diagnostic reconnect with new connection
generation and unchanged faulted persistence epoch; that connection cannot save.
Normal admission requires checked exact +1 service/broker epoch and session advance,
all-client drain, exhaustion preflight and successful resume/advance handshakes as
already specified. Partial handshake stays fenced. No extra recovery loop or
fault-clearing command is introduced. The record's old epoch/revision are historical
labels; reboot restoration does not recreate a live operation identity.

Admission errors remain the reviewed FE10/21/22/24/27/31 mapping; FE24 only for
known public denial, never predicted fresh Q or future wear-permit failure. The
retained reason always belongs to the accepted predecessor. Fresh Q/permit errors
after acceptance follow the Failed/Indeterminate rows. Host disables pyXCP automatic
error handling in its dedicated process and never interprets empty successful
response body `b''` as failure. New CONNECT must revalidate build/schema/profile/A2L.

## Review decision and release limits

The exact unresolved **standards** question is whether boot-only self-reference
is permitted by the applicable licensed XCP page model, and how that specification
requires accepted-but-unfulfilled requests to end. The inspected public sources do
not establish those answers. The request for this review is therefore explicit:
approve this restricted application policy for the pinned-client demo, or reject
its single-page interpretation and require a separately designed initialization
segment model. This candidate chooses the former; it is not offering an ambiguous
runtime switch, silently declaring compliance, or asking for further vague research.
No endpoint may claim full XCP/ASAM conformance from this package.

Independent review must accept or revise these two policies before I6 authoring.
[Integration tests](integration.md) then gate actual endpoint release. I5/T06/T36,
T16 backend qualification and native/MCU/HIL remain separate. Declared extra memory
is unchanged5120/6144; carried portable7960/8192 remains source-bound, not new runtime
measurement. Source-bound evidence, actual client observations and policy choices
are separated in [reference-evidence.md](reference-evidence.md).
