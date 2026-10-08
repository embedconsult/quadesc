# XCP scalar S0 package specification

This package implements the packet-level subset selected by contract
`led-xcp/0.1.0` in immutable bundle `contracts-s0-v0.1.1`. It is a scoped
interoperability profile, not a claim of full ASAM XCP conformity.

## Packet contract

The core accepts one complete owned XCP command PDU of 1 through 8 bytes and
produces at most one owned response PDU of 1 through 8 bytes. Serial/CAN
framing, counters, checksums, idle recovery, and device I/O are outside this
crate.

| Command | Request | Success/defined response |
|---|---|---|
| CONNECT | `FF 00` | `FF 01 00 08 08 00 01 01` |
| DISCONNECT | `FE` | `FF` after owner/session quiescence |
| GET_STATUS | `FD` | `FF 00 00 00 00 00` |
| SYNCH | `FC` | `FE 00` |
| SET_MTA | `F6 00 00 ext a0 a1 a2 a3` | `FF` when `ext=0` |
| UPLOAD | `F5 n` | `FF` plus `n` bytes, `1 <= n <= 7` |
| DOWNLOAD | `F0 02 lo hi` | `FF` only after confirmed owner `Applied` |

All multi-byte application data and addresses are little-endian and address
granularity is one byte. MAX_CTO and MAX_DTO are both 8. CONNECT advertises
only CAL/PAG (`RESOURCE=0x01`) and `COMM_MODE_BASIC=0`. Every other command is
`ERR_CMD_UNKNOWN` while connected. While disconnected, packets other than a
syntactically valid or malformed CONNECT are ignored.

## Session states and effects

The transport-neutral state model has these leaves:

```text
Disconnected -> Synchronizing -> Ready -> AwaitingOwner
                                      \-> Resolving
Ready -> Quiescing -> Disconnected
any uncertain/correlation failure -> Recovery
Recovery -> Synchronizing only through a fresh CONNECT
```

Synchronous processing emits an owned `ProviderRequest` into a caller-owned
bounded provider port. It never awaits. `SubmitError::Busy` proves nonadmission and leaves MTA and request identity
unchanged. `Ok` and `SubmitError::Unavailable` both require durable pending
correlation and irrevocable identity consumption: Unavailable allows that the
owner received the command despite a lost admission acknowledgement. It fences
wire delivery, never reports rejection, and blocks replacement until definitive
reconciliation. Queue-full maps to `ERR_CMD_BUSY` only while delivery is allowed.

Provider completions carry the exact session generation and sequence. A
completion with a mismatched key fences the session without a wire error.
DOWNLOAD advances MTA by two only for `Applied`. The core records `Applied`
before requesting `ReleaseOutcome`; if release admission is temporarily full,
it retains one retirement record and blocks another mutation until retry or
acknowledgement. `Retired`, `StaleOperation`, and `Uncertain` never become a
fabricated negative response.

The provider adapter must install the acknowledged owner view before it emits
an `Applied` completion. Thus a positive DOWNLOAD response means software
commit plus guaranteed bounded output admission, not GPIO or optical
completion.

## Checked virtual map

The map contains sorted, non-overlapping logical regions. Each region names a
stable descriptor ID, start, fixed byte length, and access policy. Resolution
uses checked `u32` arithmetic and returns descriptor ID plus bounded offset;
it cannot create a Rust pointer.

- UPLOAD may read a subrange contained in one region.
- DOWNLOAD must start at, and exactly cover, one writable complete scalar.
- Known read-only writes are `ERR_WRITE_PROTECTED`.
- Invalid scalar start/width/count and arithmetic overflow are
  `ERR_OUT_OF_RANGE`.
- Unmapped or cross-region access is `ERR_ACCESS_DENIED`.
- Failed transfers do not advance MTA.

## Resource bounds

There is one command in flight, one retained retirement, one MTA, fixed
8-byte packets, and fixed request/completion enums. The runtime path uses no
`alloc`, raw pointer, `unsafe`, OS, runtime, HAL, motor, LED, or AM13 API.
Profile D/DAQ, pages, STIM, programming, security, persistence, block mode,
optional discovery, automatic A2L transport data, SxI, and CAN are absent.

## Public reference boundary

Command identifiers, public error identifiers, and client method layouts are
checked against unmodified pyXCP 0.29.18 commit
`016cf3e44364e9cd93966d144a39d342578a0391`, particularly
`pyxcp/types.py` and `pyxcp/master/master.py`. The public ASAM overview is
<https://www.asam.net/standards/detail/mcd-1-xcp/wiki/>. Independent wire
execution uses stock pinned pyXCP and must not use this decoder as its oracle.
The T08 remediation capture uses a mock owner and PTY/SxI bridge; T10's A2L
harness and T34 product framing remain independently owned.

## Durable fencing and lifecycle

Every admitted Synchronize consumes a monotonically increasing attempt generation,
including Busy, failed or lost attempts. The active generation is installed only
on a matching, reply-enabled Ready. Exhaustion fences without wrap. No CONNECT
may replace pending owner/control work or unacknowledged outcome retirement.
A lost attempt is reconciled silently before a new CONNECT is admitted.

Loss, sequential-command violation, mismatched completion and action/response
capacity failure durably revoke wire delivery. They preserve the exact pending
request and retirement state. Writes continue ResolveOrCancel admission retries;
matching Ready/Busy/read/quiesce completions clear old work silently and never
reactivate it. Uncertain outcomes remain unresolved; no fabricated rejection.
Retired/StaleOperation prove the matching key fenced, never historical rejection.
Fresh synchronization/readback is required before another writable session.

The response slot is transport-scoped. Loss/fault/Stop/Reset invalidate unsent
responses, including successful or failed writes, without discarding owner truth.
Pure guards observe the core result after synchronous actions have fenced it.
Session exit fences Stop. Initial Disconnected entry always fences on Start/Reset,
which skips Init and may skip the shared Session exit. A separate Closed HSM leaf
represents normal protocol disconnection (core Disconnected) and keeps its response.
Thus Reset also invalidates an unsent DISCONNECT response from Closed. Hooks never reconstruct resources.
Completions must remain durably owned by the adapter while the actor is stopped
(the pinned engine drops domain messages in Init); replay them after Start.

Quiesce commands/completions use `Correlation { session_generation, sequence }`,
consuming the same nonwrapping per-session sequence as reads/writes on admission.
A retry following Busy therefore cannot accept a delayed earlier Quiesced result.
The concrete adapter must preserve these coordinates in its control completion,
just as it validates service_epoch before forwarding write/read completions.

## Uncertain admission correction and verification

The same initial admission transition governs Synchronize, Read, Apply and
Quiesce. Preserve the precise generation/correlation/key and completion metadata
on both confirmed and possible admission; consume its identity exactly once.
Only known nonadmission permits reuse. Initial requests are never resubmitted.
A late matching Ready cannot install an active epoch after its delivery fence.
Read(Uncertain), Synchronize(Uncertain) and Quiesce(Uncertain) remain pending
until a definitive matching completion. A late Applied is recorded and released
under its original operation key, with no wire response.

ResolveOrCancel and ReleaseOutcome are canonical idempotent control operations.
Unconfirmed admission can retry under the same exact key; confirmed admission
waits for completion. Track possible admission independently of retry readiness:
a Busy retry does not undo an earlier uncertain handoff. A matching Released
after possible admission clears retirement; a Released before any possible
admission is a correlation fault. Repeated faults preserve all of this state.
No wire response escapes a durable fence until a fresh confirmed synchronization
is admitted and its matching Ready completes.

R2 verification runs the five independently written uncertain-admission tests
first against the pinned failing base, then corrected source. Extensions cover
late definitive outcomes, uncertain resolve/release followed by Busy, identity
exhaustion/non-reuse and actual generated lifecycle/saturation/loss actions.
EffectSlot returns only Ok/Busy; generic Unavailable is exercised through the
public Session ProviderPort, not asserted to originate from EffectSlot.

## Bounded service result (downstream candidate revision)

`Session::service` returns `ServiceResult { dispatch, new_fault }`. Dispatch
preserves prior response/durable-fence semantics; new_fault is true only for
Unavailable observed by this call's ReleaseOutcome or ResolveOrCancel attempt.
It is false for Busy, success, confirmed admission, no work, and a pre-existing
durable fence alone. The field is step-local and needs no reset or exhaustion
state. Transport consumers must invalidate a recovered/partial boundary for a
new fault even while durable delivery remains fenced. The packet-only context
uses dispatch to synchronously clear its response slot on every durable fence.

Release failures retain the exact operation and uncertain admission; only actual
matching Released clears retirement. Resolve failures retain the write, revoke
reply permission and never re-submit Apply. New-fault reporting does not execute
an extra control step. Initial packet/completion/input/lifecycle faults continue
to propagate through their existing fenced dispatch. All actions remain bounded,
synchronous and allocation-free. The accepted historical T08 input is immutable;
this API is an unaccepted downstream revision requiring independent review.
