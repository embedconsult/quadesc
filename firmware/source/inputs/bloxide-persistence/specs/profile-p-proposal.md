# Separate later profile P — public evidence and release boundary

Status: review candidate. **P wire activation is deferred**, while the portable
record/service design can be reviewed and implemented independently. S0 continues
to reject SET_REQUEST (F9) with ERR_CMD_UNKNOWN (FE20); no save register is added.
P is a new matched firmware/profile/A2L bundle, never an extra bit silently enabled
in S0. This is a scoped interoperability proposal, not full ASAM conformity.

## Evidence-backed subset

NI's public ECU M&C manual describes status bit 0 as a pending calibration-store
request, cleared when fulfilled, with an optional completion event. Its segment
API also relates FREEZE mode to storing calibration. It does not specify the full
negative/timeout behavior needed here. See the original publisher's
[371601F manual](https://download.ni.com/support/manuals/371601f.pdf), printed
pages 5-150/151, 5-160/161 and 6-76/77; local file/hash is in sources.json.

Pinned unmodified pyXCP 0.29.18 commit
016cf3e44364e9cd93966d144a39d342578a0391 exposes Master.setRequest(mode,id),
command F9, STORE_CAL mode bit 0, GET_STATUS.storeCalRequest bit 0 and
EV_STORE_CAL code 03. Its setRequest test constructs F9 15 12 34 for arguments
(0x15,0x1234). The implementation explicitly packs the ID big-endian, even when
negotiated byte order is little-endian. This is a client fact, not authority for
all XCP versions. [Pinned master source](https://github.com/christoph2/pyxcp/blob/016cf3e44364e9cd93966d144a39d342578a0391/pyxcp/master/master.py),
[types](https://github.com/christoph2/pyxcp/blob/016cf3e44364e9cd93966d144a39d342578a0391/pyxcp/types.py),
[test](https://github.com/christoph2/pyxcp/blob/016cf3e44364e9cd93966d144a39d342578a0391/pyxcp/tests/test_master.py).

Only zero configuration ID is proposed for P (no DAQ configuration save). Thus
`setRequest(1,0)` sends CTO **F9 01 00 00**, independent of that endian discrepancy.
Do not patch the client or use a nonzero ID until independent primary evidence
settles its semantics. CTO/DTO max8, byte granularity, little-endian scalar map
and qualified SxI transport carry forward by a new versioned profile. P has no
PGM, DAQ, STIM, block, seed/key, discovery or physical CAN claim.

## Exact candidate mapping, subject to P review

The following is selected application behavior for future implementation/testing;
only the facts identified above are evidenced as standard/public-client behavior.
All negative-response choices remain proposed until the P wire review below.

| Condition | Candidate behavior |
|---|---|
| Connected/Ready, exact F9 01 00 00, no unresolved save, policy/capacity/wear eligible | At C capture complete validated owner snapshot; return FF only after Accepted retained. No flash durability implied. |
| Accepted, storage pending | GET_STATUS response FF 01 00 00 00 00 (six CTO bytes): pending bit only, protection0, reserved0, session config0. |
| Exact new record fully verified or exact DurableExisting | Record retained Durable; status pending clears: FF 00 00 00 00 00. Optional EV_STORE_CAL is **not emitted** in this first proposed P cut. |
| Wrong F9 length | Candidate FE21 ERR_CMD_SYNTAX, no capture/media command. |
| Mode !=1 or ID !=0 | Candidate FE22 ERR_OUT_OF_RANGE, no capture/media command; includes mode0 and mixed DAQ bits. |
| Another unresolved save, completion capacity full, rate limit | Candidate FE10 ERR_CMD_BUSY, no capture/media command. |
| Unsupported schema/ambiguous media/wear exhausted/known quarantine before C | Candidate FE32 ERR_VERIFY, no capture/media command. Detailed application reason through read-only P diagnostics. |
| Lost FF response | Client cannot know whether admitted. Never auto-retry F9. Resolve status/readback on original observed connection; a new wire F9 is a new request (wire has no operation key). |
| Post-accept storage failure/timeout | Portable outcome Failed or Indeterminate is defined. **Standard wire indication/clearing policy deferred**; do not ship P with a made-up success or event. |
| Disconnect/reset while pending | Portable operation continues or enters uncertainty reconciliation; no replay to a new connection. Quiesce admission and reconcile before new session Ready. MCU reboot requires fresh readback. |

Even successful bit-clear alone does not identify the saved bytes. Client workflow:
validate matched P build/schema/A2L; read active values/revision; issue one request;
record accepted response; poll status (max100 polls at 10ms in host fixture,
configurable bounded target deadline from T16); require success diagnostics and
full durable snapshot equal captured snapshot. A RAM DOWNLOAD after acceptance
may advance RAM revision/values; durable readback must still identify captured
values. After timeout/disconnect, report unknown and reconcile, never "not saved".
After observed MCU reboot, normal active readback must equal the durable snapshot,
with boot-local active revision0; do not compare saved revision to boot revision.

## Readback interface proposal (application data, not a private save command)

Before P activation the composed application must allocate a new nonoverlapping
read-only descriptor block and a new schema/profile identity. No addresses are
allocated into accepted S0 by this design. Names and payload layout below are
concrete for the independent fixture; composition supplies base address B.

Single `persistence.view` is a 320-byte immutable session read snapshot:

| Relative offset | Bytes | Meaning |
|---|---:|---|
| 0 | 8 | view_generation (monotonic within live persistence epoch; exhaustion fences publication) |
| 8 | 8 | persistence service epoch |
| 16 | 8 | accepted operation sequence, zero when no observed operation |
| 24 | 8 | current durable record sequence, zero when no record |
| 32 | 8 | durable source RAM epoch, zero when absent |
| 40 | 4 | durable source RAM revision, zero when absent |
| 44 | 4 | state: 0=no record, 1=pending, 2=durable, 3=failed, 4=indeterminate, 5=read-only fault |
| 48 | 4 | reason: 0=none, 1=backend, 2=verify, 3=wear, 4=version, 5=ambiguous, 6=timeout, 7=ECC, 8=policy |
| 52 | 2 | durable schema ID, zero absent |
| 54 | 2 | durable payload length (0..256) |
| 56 | 4 | accepted session_generation (zero absent) |
| 60 | 4 | reserved zero |
| 64 | 256 | durable payload then FF padding; all FF absent |

All fields LE. SET_MTA(B) latches one copy into preallocated per-client storage;
UPLOAD chunks of <=7 bytes read that same copy to offset320, then retire the
latch. Repeated SET_MTA(B) replaces the client's copy; mid-block MTA without an
active latch rejects. Four client latches require an additional 1280-byte budget
in P adapter, outside portable service budget. Existing S0 scalar read semantics
stay unchanged. Future exporter must declare this as application read-only blob/
scalar metadata consistent with its parser-supported descriptor format; it is
not an XCP standard status extension. Writes to any byte reject FE24.

This view reports accepted key and current durable data. During Pending it shows
previous durable data; compare state as well as bytes. Captured pending bytes are
retained internally and delivered in the broker's accepted result; standard F9
cannot carry them. A single XCP client's C is after its preceding acknowledged
DOWNLOAD and before its next command. For other concurrent writers the broker
must return its C snapshot to its own client; wire P client must serialize external
writers for expected-before-save comparisons, or accept the observed captured
revision without falsely claiming its pre-save read was the capture.

## Narrow unresolved items / release tests

P-OPEN-1: Public material inspected does not settle which segment/page/FREEZE
setup is required for a standard store of this checked virtual map. Do not declare
a permanently frozen page or invent SET_SEGMENT_MODE behavior. Obtain a public
primary implementation/manual establishing the selected single-segment behavior,
then write exact page/segment command/profile/A2L tests if needed. This may expand
P only, not S0. No paid specification purchase is required.

P-OPEN-2: Establish from public primary evidence the late storage-failure wire
status, request-bit disposition, session continuation and any event payload.
Candidate negative command mappings above also need independent wire review.
Until then portable Failed/Indeterminate is testable locally but P cannot activate.

P-OPEN-3: Independent unmodified pyXCP tests must validate raw bytes AND decoded
fields, no upstream monkey patches, against the actual endpoint/storage service.
Run pending/durable/failure, lost reply and reboot/readback paths. A mock transport
encoding test does not satisfy interoperability. Pin pya2ldb1.0.353
c19c3ad2f285d1230e334bad81eeb09cbaaa3031 unchanged for any P metadata extension.
This design does not fabricate a dependency lock or client test result.

These are technical review dependencies, not new user product choices. The next
bounded portable implementation can proceed after design approval without P wire
activation; report the original protocol acceptance clause unmet until resolved.
