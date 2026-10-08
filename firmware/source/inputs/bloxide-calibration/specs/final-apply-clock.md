# Final apply clock binding — T07 F1 correction

This implementation specification binds the existing immutable
`led-calibration-ownership-v0.1.md` deadline/phase policy and
`led-output-slot-binding-v0.1.md` reservation cancellation policy. It does not
revise either contract.

`Owner::apply(request, clock, make_candidate, gate)` accepts a `FnMut() -> u64`
hook instead of a pre-sampled timestamp. The hook is a trusted bounded,
synchronous, infallible, allocation-free read of current monotonic microseconds
in the deadline domain, without wrap during the live service. Sampling a saved
entry timestamp in this hook violates its contract. Borrow platform time from
the owning integration; do not embed runtime/HAL resources in owned messages.
The hook may be passed by mutable reference to avoid destroying owned resources.
Hook/candidate/gate captures must obey the no-heap-after-freeze lifetime rule.

Decision order:

1. Return existing retained/fenced/busy/lifecycle outcomes without running hooks.
2. Sample entry clock; reject expired requests, then check expected revision.
3. Construct and validate the full candidate; validation errors remain retained.
4. For same-value: sample clock again, reject at/after expiry, otherwise retain
   Applied(changed=false) with this sample. No slot, revision or phase change.
5. For changed-value: attempt reservation; retain its existing admission error
   on failure. Once reserved, sample clock again immediately before mutation.
6. At/after expiry, drop the still-unpublished permit and retain Expired without
   active/revision/phase/output changes. Drop must cancel Reserved to Empty.
7. Otherwise commit active/revision, publish infallibly with that final sample
   in CommitRecord, then retain Applied with the identical sample. Component
   publication uses that record timestamp as its new phase epoch.

Exactly two clock reads occur on a validated successful/no-op or final-expiry
path. No clock read occurs after mutation begins. The comparison and commit are
synchronous; this library cannot qualify platform interrupt latency, clock
accuracy, real-time deadlines or runtime slot safety. No new runtime or HAL
abstraction or legacy timestamp API is introduced.

Normative regression: tests/deadlines.rs, covering validation/reservation
advances to deadline 100 and beyond 101, no-op rejection and fresh success,
just-before-expiry 99 publication/phase/outcome, Drop cancellation and retained
results. Existing transaction tests cover release/reset/cancel fencing.
