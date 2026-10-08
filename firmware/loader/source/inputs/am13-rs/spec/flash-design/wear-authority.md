# Reset-surviving wear and quarantine authority audit

R8 and portable-correction-r1 are unchanged: authority-issued lease/receipt,
durable debit before grant/erase publication, consume once before physical erase,
no refund after failure, per-slot budget and stable quarantine. A RAM counter,
record sequence, MCU service epoch or boot ID cannot satisfy these requirements.
Two records do not record an erase interrupted before commit. SRAM retention,
unused NONMAIN, OTP/fuses and a guessed hardware monotonic counter are not options.

## Feasible bounded demo design, still unimplemented/unqualified

Use an external durable authority under the future serialized T04 custodian,
outside the MCU reset domain. It serves reviewed fixed wear messages over the
existing authorized demo transport integration; it does not give the user a raw
flash/erase command. No service installation, credential or bridge modification
is done or authorized by this design. T17 wire P decisions and T04 service/native
gates must be resolved by their owners before this seam is operational. This is
an additional backend dependency, not a replacement for the unchanged T17 API.

A provisioned finite ledger is keyed by physical device identity, provisioning
generation, geometry hash, physical A/B bank+sector, reviewed image/driver profile,
allowance and authenticated authority instance. Counter domain is external, checked
u64, nonwrapping, never recreated on MCU boot. Persist:

- remaining W_A/W_B and device-wide pump budget W_total;
- non-reused lease number and retirement watermark;
- finite receipt table keyed by (lease,issuance,slot), request digest and receipt;
- durable debit, consume/intent, settled result and quarantine flags/diagnostics;
- independently reconciled existing erase/program history and any provisioning,
  BSL recovery or test use charged against the same device/pump allowance.

W_A+W_B plus all other device use must fit a conservatively derated, documented
pump allowance. Minimum20k from TI-A is not20k per sector simultaneously without
limit. The table does not fully explain mixed program/erase pump accounting;
obtain the applicable vendor accounting/qualification rule. Until historical
usage, device grade and that rule are qualified, available allowance is **zero**,
not a manufactured fresh100 or200 count. Cap normal saves at a finite reviewed
number; do not refresh it on reconnect/reset. Rate limiting is additional load
control and cannot replace accounting. A new generation requires separately
reviewed provisioning that preserves previous lifetime consumption.

Finite storage design: reserve N receipt entries upfront, at most one active
operation; e.g. budget an upper bound1024 durable bytes per authorized permit plus
4096 metadata bytes (proposal, not actual fs allocation/performance proof). N is
bounded by approved W_A+W_B; all file/WAL, filesystem metadata, backups and database
own write endurance belong to the authority's resource qualification. No implicit
unbounded audit growth; when full, refuse. External host allocation is not MCU
post-freeze allocation, but MCU transport buffers are fixed. Actual durable
format/checksums/locking/commit protocol must be implemented and independently
fault-tested before the first WearGranted.

## Linearization and crash cases

| Transition | Must be durably committed before response/action | Crash/replay consequence |
|---|---|---|
| Lease establishment | Old lease fenced, new non-reused lease bound to physical provisioning and sole reconciled MCU session | Old receipts stay spent; no old-session packet may authorize a new boot |
| Acquire (lease,issuance,slot) | Debit per-slot AND device budget; write immutable receipt row | Repeated exact acquire replays same receipt, even if consumed; disagreement rejects. Lost grant may waste credit, never recreate it |
| Consume receipt for Erase | Atomically transition issued->consumed with pending operation intent, bind exact header/profile/target | Only fresh committed transition permits one hardware admission in current live owner; duplicate consume returns spent, not another execute permission |
| Any erase/body/marker activity | Erase-associated pending save intent remains durable through final verification | Disconnect/reset/death means affected slot remains uncertain; no new lease/save before reconciliation |
| Settled verified save/failure | Record terminal evidence and quarantine state; cleared pending flag only if qualified idle/full read evidence permits | Lost terminal ack replays outcome. Failed or indeterminate media is never auto-cleared |
| Quarantine update | Record slot/device fault and fence all future writes | If authority unreachable, current owner locks locally; all rebooted MCUs start write-disabled until authority reconciles |

Actual hardware issue follows durable consumption exactly once inside the same
live backend. If consume response is lost before issue, the credit remains spent;
do not ask authority to return a second execute permission. If MCU crashes after
consume and before issue, intent cannot distinguish no erase from partial erase:
quarantine and charge anyway. If authority crashes after fs commit but before
reply, exact replay is observation only. Absence/torn ledger, rollback, duplicate
authority, mismatched profile or stale lease denies all writes. A checksum does
not detect restoration of a valid old backup; require a qualified anti-rollback
custody/generation mechanism or fail closed after uncertain authority restoration.
No claim that plain fsync on an unqualified storage device proves power-loss
persistence; use native fault campaigns plus underlying storage guarantees.

The port needs a fresh MCU session binding that cannot accept buffered pre-reset
grants: serialized custodian observes/reset-fences connection, drains old transport
bytes/commands, retires old lease, and verifies the new exact ready identity before
issuing another lease. An arbitrary volatile nonce alone is insufficient. Loss of
that binding or unsolicited MCU reset disables all writes until the externally
observed fence/reconcile procedure succeeds. This session/transport mechanism is
an explicit implementation/review gap; ordinary reconnect is not proof of reset.
No cryptographic protocol or provisioned key is invented here.

Power-cycle restoration of already committed LED settings must work offline via
qualified boot scan. **New saves require the qualified online authority** in this
bounded demo candidate. This satisfies a connected XCP demo without claiming
standalone persistence writes. If independent offline saving is later required,
that requires an independently reviewed durable backend authority. An internal
MAIN ledger would introduce its own wear/crash/GC/isolation problem and extra
regions; it is not justified by these two records and is outside this cut.

A precharged finite allowance in MCU RAM is insufficient across resets without
nonreplayable external session/receipt enforcement. A host simulator that keeps a
receipt table alive across simulated MCU replacement proves only its own model;
it does not qualify host-process crash, power failure, flash pump wear or the real
link. No simulation tests are relabeled physical authority acceptance.

## Recovery authority is separate from Q

Boot may select the remaining valid record, but external authorization to reuse a
bad slot never waives Q/ECC health or monotone-model requirements. A quarantined
slot remains quarantined after late success/logical Reset. No administrator can
change a bad marker/pair to manufacture an erasable prestate through ordinary
save. If full read Q cannot be proved, two-slot writes stop. Reprovisioning and
MAIN recovery must go through separately reviewed serialized T04, with all
consumption preserved and no NONMAIN/security changes. Whether old/new/defaults
are reported is determined by portable recovery; ledger does not elect a record.
