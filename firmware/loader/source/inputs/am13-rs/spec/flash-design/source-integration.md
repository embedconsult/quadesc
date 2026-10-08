# T16 R1 conditional source seam

`src/flash.rs` owns the only `FlashController` moved from `Peripherals`. It has no
`Clone`, `Copy`, raw-address API, global actor or write-capable production
implementation. `Qualification::unqualified()` is the only production constructor.
`FlashController::issue` returns `Unqualified` without MMIO. Thus this source
cannot be installed as a write backend. In particular, a host `Controller` mock
and all-true test-only qualification are state-machine tests, not target support.
No production register masks, FRI/RWAIT values, timeout constants or wear
allowance are selected by this source.

The private `src/flash/registers.rs` now records a conditional NVMNW/GSC
register sequence and host register-model tests. Its constructor and progression
hook exist only under `cfg(test)`; `FlashController` never calls it. The model
uses SDK-derived candidate mask bits 10/11 and full-word ECC byte enables, while
requiring an explicit test-only bank-READ assertion and observed INPROGRESS
transition before accepting DONE. It cannot supply a physical idle/drain
assertion, deployable deadline, read/ECC attribution or production qualification.
The qualification and driver-contract documents in this directory state the conditional register semantics.

The model retains its last consumed `Header` in the existing fixed header slot
after ordinary or late cleanup. Within one `Sequence` lifetime, `begin` rejects
the same or an older sequence and a different epoch before any register access;
a greater sequence in that epoch may start after cleanup. Failed cleanup keeps
the active fault fence. A new epoch requires a newly qualified controller
lifecycle rather than an in-place identity reset. This is model-local replay
custody, not a second production owner or durable reset history. `FlashOwner`
continues to own the retained failed completion: late status and cleanup do not
upgrade a timeout to success. The private model has no production constructor.

The portable persistence bridge is a no_std interface pattern outside the concrete
application calibration backend. Its
`command`, `completion` and `quiesce` mapping is exhaustive over the public
`BackendCommand`, `CommandHeader`, `CommandKind`, `CompletionStatus`, `ReadIssue`,
`WearLease`, `Slot` and `QuiesceRequest`. The composition owner must bind these
conversions to one fixed command slot and one independent quiesce slot. The T17
service publishes a command through `take_command(now)`; composition captures
that publication time and the qualified absolute deadline from its original
`BackendTiming`, then moves the entire owned 256-byte payload to `FlashOwner`.
Never restart its budget after queueing. Call service `advance_time(now)` from a
live monotonic source even while the controller is busy; `FlashOwner::poll(&mut clock)`
uses the same domain. Never treat a local timeout as idle. A completion is retained
until full-header acknowledgement and published quiesce settlement. A logical
Reset does not reconstruct Peripherals or an external lease.

`FlashOwner` records the complete command and sequence before `Controller::issue`.
An `issue` error may follow physical execution, so it retains a failed,
unquiescent completion and fences the owner. Exact replay does not issue again;
changed payload/header and unrelated work are also refused. The future concrete
controller must report `Done` with the exact issued header. `Done` alone is a
status observation: a matching, kind-valid terminal `Ok`, `Read` (including
read issues), `WearGranted`/`WearDenied`, or `Failed { quiescent: true }` is
the controller's explicit qualified idle and completion-drain assertion.
`Failed { quiescent: false }`, a mismatched header, `Busy`, `Fault`, timeout and
protection restoration alone cannot settle custody. Cleanup must succeed too.
After uncertainty or a missed deadline, the retained result stays failed even
after qualified late completion, and only matching quiesce permits acknowledgement.
No controller implementation currently supplies the physical proof behind these
assertions; this is the interface a future reviewed implementation must meet.

T17 owns Q/complement classification and must perform the full inactive-slot
qualification before publishing a permit command. The actual external durable
wear authority must debit/grant and consume/persist intent before erase, with
whole-device and per-slot accounting. `external_authority_ready` is only a local
admission signal, not proof of such an authority. With missing history the
allowance is zero. F-ID/F-GEOM/F-MODEL/F-ECC/F-POWER/F-RAM/F-TIME/F-WEAR/
F-INTEGRATION are unqualified, so no application may construct a write profile.
A future concrete constructor and controller must be independently reviewed,
including exactly one grant/consume flow and all failure windows.

A production controller must execute from a linked SRAM0-2 island, including
transitive callees, literals, veneers, panic/fault paths, diagnostics, program
buffers, vector table/handlers, time source, short critical sections, stack and
watchdog feed path. It must verify qualified protection/readback before execute,
perform exact 16-byte subcommands for G256, capture status/ECC before clear,
restore and read back protections and NOOP, and refuse uncertain reuse. No
interrupt masking across erase; preserve 100us time and UART liveness. SRAM3
is excluded. `src/clock.rs` does not verify FRDCNTL. The TRM/datasheet RWAIT
conflict remains unresolved; no proposed setting may violate the current
SPRSPC3A minimum1. If a future FRI change is required, run its RAM procedure,
including no covered flash access and nine-cycle propagation, after final target
qualification. Typical programming times do not supply finite deadlines.

Before accepting any concrete implementation, inspect final ELF PT_LOAD file
intervals and section VMAs for bank0 load/execute and the complete RAM relocation
and call closure. Require exact target disassembly, ISR/DMA accesses, stack and
all-four allocator-entrypoint traces from first poll through reset/fault/cleanup.
`probes/flash_ram_closure.py` is a fail-closed first pass over a final ARM ELF:
it rejects file-backed bank1 loads, allocated addresses outside bank0/SRAM0-2,
orphan allocated sections, incomplete/malformed PT_LOAD mappings, and a missing,
unloaded or misplaced nonempty `.ram_flash` island. Every nonempty island needs
consistent PT_LOAD SRAM VMA, file offset and bank0 file-backed LMA coverage.
The linked positive control and unloaded review fixture exercise both outcomes.
It always reports
`ram_closure_proved=false`; manual transitive analysis and exact linked image
review are still mandatory. The prior T05 fixture and an x86 ELF are retained
negative controls, both rejected by this new probe.
Arithmetic geometry checks do not establish RAM closure, watchdog, voltage, ECC, interruption or wear guarantees. Unsupported generic backend paths remain disabled until their qualifications are established.
# Read-only startup snapshot (2026-09-25)

`FlashController::observe` now captures the factory MAIN size/bank field,
FRI read-control/interface-control, NVMNW status and dynamic protection words
without touching MAIN data or writing a register. `basic_readiness` is only a
necessary diagnostic predicate: 512 KiB/two banks, RWAIT>=1, no in-progress
command and NONMAIN dynamic protection. It cannot authorize reads or writes.
The FRI interface offset 0x100C comes from SDK `hw_fri.h` SHA-256
3189ea6fafdc57eb7e06d466691aa8d4f579f7e52c9eed73622ad4c5663d5488;
TRM and `hw_nvmnw.h` hashes in `primary-evidence.md` were rechecked. UART
completion from `c461d91` is already in this branch ancestry, with no copy.
