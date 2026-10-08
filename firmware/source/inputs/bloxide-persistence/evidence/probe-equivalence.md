# Normative test API equivalence

The portable correction at revision `9e8a74f5dedc94639f6aebabe05d876b6bac18f7`
retains the original normative assertions while adapting public API setup.
The original source revision is `ff4658819bf4e82a80d2f2060881c65fd81fab9a`.
No assertion was weakened to accept a previously rejected behavior. The retained
technical equivalence is described below; historical execution streams and
alternate comparison workspaces are excluded from the customer package.

- `save(..., now)` becomes `save(..., || now)`: the same constant time is returned
  at both fresh samples. New tests separately exercise delayed validation/expiry.
- `take_command()` becomes `take_command(0)`: these original probes inject explicit
  completions without elapsed time. New tests exercise real deadline transitions.
- Constructor gains qualified `BackendConfig` timing and an authority-issued lease.
  The core-only independent Media uses a fixed single-session lease; it has never
  claimed wear-authority testing. The wear probe obtains `m.open_session()` on each
  reboot from the surviving authority; MCU epochs/sequences still restart at 1.
  This is the essential F8 correction, not a supplied monotonic MCU boot ID.
- `WearGranted { permit }` and `Erase { permit }` explicitly carry the single-use
  receipt. The core-only Media returns the acquisition command sequence as its
  local receipt and ignores it on erase, as it ignored wear in the original probe.
  The independent F8 probe uses the real simulator authority and consumption checks.
- F5's `foreign_schema_must_not_displace_previously_valid_configuration` originally
  asserted `Accepted` to enter the buggy workflow. That **setup assertion** is
  replaced by a stronger early `Rejected(InvalidSnapshot)` and zero-erases check.
  Its original normative assertion that reboot still selects the old valid record
  is unchanged. The other original foreign-schema/no-erase probe is unchanged.

The original 14 tests now all pass. Independent read injections remain 528; false
NoNewCommit changes 132 -> 0. Independent wear witness changes 101 erases/99 permits
remaining -> 100 erases/0 remaining. Core Q/codec oracle remains 125578 checks.

Product tests that expected all 99 destructive commands after a correlation fault
were contrary to R5/F7. They now require termination before new destructive work or
post-marker read-only reconciliation. Their original versions remain at the base
Git commit; unchanged independent F7 assertions and new all-position fault tests
validate the correction. Synthetic local wrap/results-count tests were replaced by
actual public Owner wrap and real service/broker workloads, not counted as coverage.

The external-consumer campaigns use an independently written raw CRC/envelope
oracle. They provide software model coverage rather than physical qualification.
The current package carries required tests and source contracts; its build and
regression outcomes are recorded in `firmware/build-report.json`.
