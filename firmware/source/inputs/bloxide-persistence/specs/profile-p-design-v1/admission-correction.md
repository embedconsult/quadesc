# F1/F2 correction: API boundary and retained diagnostics

This is a design trace, not executed Rust or endpoint behavior. Source authority
is unchanged700df762; source hashes and literal API ties are checked by
`tools/profile-p-design-v1/check_admission.py`. No atomic preflight is introduced.

## Known inputs and precise response boundary

The adapter may read calibration `Owner::mode`, registered `Schema` (including
`Snapshot::validate_schema`), `Persistence::mode/state/health`, fixed broker/adapter
capacity and its own FREEZE/connection/policy/deadline state in a serialized turn.
`health().write_locked` is the public latched denial fact; quarantine flags alone
are not a substitute for that write-lock decision. Unsupported registered schema
can be rejected before capture; invalid captured data can be rejected only when
actually validated. There is no public fresh-Q/wear-preflight input. In particular
previous WearDenied does not itself set write_locked: do not invent a wear latch.
A failed accepted request remains fenced under the candidate session policy.

Known-lock trace (all earlier precedence conditions eligible): observe
`health().write_locked == true` -> generic FE24. No Broker reserve, capture_owner,
save or take_command for this request; no new key/C. Existing summary remains.
This is adapter policy based on a real public input, not a claim that calling
save on a faulted service returns WritesLocked: save checks lifecycle first.

Eligible write trace: Broker::reserve(client) gives key K; capture_owner produces
validated snapshot S into existing scratch, without changing the predecessor
summary. `Persistence::save(K,S,deadline,clock)` samples the final deadline,
installs Pending with its owned record snapshot, resets scan, enters QualifyErase
and returns Accepted. Publish exact K/C from S to the bounded summary, then FF.
Only then pump `take_command(now)`/correlated `complete(Completion)` outside the
synchronous action. C is never recaptured from later RAM edits. Fast
Terminal(Durable{existing:true}) is separately a successful accepted save without
these write-path commands; it does not prove a fresh permit was available.

| API continuation after Accepted/FF | Actual outcome / candidate reason | Ownership / wire |
|---|---|---|
| Read full target; fresh classification differs from cached identity | Failed{phase:Qualification, reason:RecoveryChanged, no_new_commit:true}; candidate reason9 | Settled; no permit/erase, no second F9 response |
| Read full target; unchanged classification but Q=Unsafe | Failed{Qualification, UnsafeErasePrestate, no_new_commit:true}; reason9 | Settled; no permit/erase, no second F9 response |
| Read with a reported ReadIssue | Failed{Qualification, Read(issue), no_new_commit:true}; reason7 for CorrectedEcc/UncorrectableEcc, reason1 for Io | Settled; no erase, no second F9 response |
| Fresh Q safe; AcquireWear -> ConsumeErasePermit{lease,issuance}; WearDenied | Failed{Wear, WearUnavailable, no_new_commit:true}; reason3 | Settled; no erase; no second F9 response |
| Qualification/permit command Failed{quiescent:true} | Failed{current phase, Backend, no_new_commit:true}; reason1 | No new commit; no second F9 response |
| Qualification/permit command Failed{quiescent:false} | Indeterminate{current phase, Backend}; reason1 | Unsettled; retain outstanding command, capture and bounded quiesce control; no record_terminal/release yet |

These are service variants, not an invented late FE24. Terminal Failed and
Indeterminate preserve exact K/C and old durable bytes in these pre-erase cases.
The proposed originating-connection status stays pending/unfulfilled. Actual
late-failure/status/new-session rules remain P-OPEN-2. Fresh safe Q must still
precede the external durable permit; Erase requires WearGranted's nonzero receipt.
No media command moves before C, no second F9 response and no automatic retry.
Timeout/correlation faults likewise require exact service reconciliation, never
an inferred preadmission rejection or inference of physical idle.

## Rejected staged admission and retirement

If capture fails after reserve, resolve the exact key; resolve_or_cancel may
produce Cancelled only for an unknown, current, otherwise unoccupied service.
If save returns Terminal(Rejected), use that exact outcome. Preserve the prior
accepted summary throughout both paths. Require service ownership_settled before
Broker::record_terminal, record the allocated key's terminal, then service.release
and broker.release after result consumption. These ephemeral rejected/cancelled
results retire the allocated key but do not replace the accepted-operation
summary. A failure of reconciliation leaves admission fenced; no fabricated FE.
The adapter already has shared scratch and bounded summaries; no fifth history,
new Broker API or new admission diagnostic storage is needed.

For an accepted operation, copy its final outcome/K/C into its summary before
service release; record_terminal only after ownership_settled. Broker result is
retained until consumption. Namespace change drains every client's settled slot
and transport suffix, preflights exhaustion and performs exact +1 broker
service-epoch/+1 generation plus service resume with newer I/O epoch. Stopped,
no retained/pending/outstanding/control work and no write lock are required. Both
handshakes must succeed before Ready; partial success stays fenced.

## F2 reason scope and counterexample disposition

Offset52 is solely the retained accepted operation's reason. Generic FE24 adds no
promise of per-denial detail. Success -> next known-lock rejection preserves the
success, reason0, exact key and capture; failure -> next calibration-owner disarmed-policy rejection preserves
that failure's original reason/key/C. The latter is FE27 at the earlier lifecycle/
policy gate, not a new failure outcome. The fixtures hold all other observations
constant, so complete640-byte before/after equality is required. In live readback,
current durable, view generation and connection observations can change for their
own reasons; they cannot overwrite retained-operation bytes.

The independent denial_reason_gap.py is retained unchanged in the correction hub.
Its attempt to put wear reason3 on a retained Durable must still exit1 at decode's
state==2/reason==0 assertion. That expected failure is evidence that the rejected
claim remains unrepresentable, not a product failure or a newly added feature.
The correction removes that claim. Added mutation controls also reject changing
a prior failure's reason (which is individually a valid view), key, C, or state.
No prior operation becomes Failed because a later F9 was denied.

P remains disabled. No C21-C23, S0 endpoint regression, I5/T06/T36 or I7/T16/native/
MCU/HIL evidence is supplied here.5120 proposed P bytes/6144 ceiling and carried
7960/8192 remain unchanged; combined13080/separate ceilings14336 are arithmetic,
not stack/runtime/native/target/full-image fit.
