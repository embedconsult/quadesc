# Owned backend interface, execution and reset contract

Proposed seam, not new Rust API or T06 generation syntax. Reuse exact public
T17 `service.rs` types rather than invent incompatible completion semantics:
`BackendCommand {header,data:[u8;256]}`, `CommandHeader {io_epoch:u64,
command_sequence:u64,slot,kind,offset:u32,len:u16}`. Kind is Read,
ConsumeErasePermit{lease:WearLease,issuance:u64}, Erase{permit:u64}, Program.
Completion carries exact header, owned256-byte data, and existing status Ok,
WearGranted{permit}, WearDenied, Failed{quiescent}, or Read{len,issue}.
`QuiesceRequest {outstanding:CommandHeader}` uses a separate capacity-one control
slot. Keep hardware diagnostics in a fixed sidecar correlated to the same header;
no added portable variant is required. One owner, command slot and retained
completion slot, one outstanding hardware operation globally. No background queue.

A future `FlashOwner` consumes a one-shot chip handle at startup; private MMIO
cannot escape or be cloned into competing drivers. The existing Peripherals
currently has no such handle: adding it is later reviewed implementation. No Send/
Sync override, global actor backdoor, borrow across messages or resource reacquire
on Drop/Stop/Reset. Board supplies verified profile/slot descriptors; composition
owns reservation and maintenance policy; portable service owns record protocol;
authority owns budgets. XCP save receipt is not durable completion.

## Admission and state transitions

1. Boot remains fenced until controller idle/all banks READ, exact identity,
   protection/cache/ECC readiness, no old completion and qualified power/clock
   predicates hold. Prove reset semantics, not software's default bool. Read-only
   recovery precedes Owner::new/Ready. New external lease is separately required
   for writes; it is not a random MCU boot number. Restore historical record labels
   separately; active RAM owner's revision begins0. Logical Reset does not reload
   storage or change active values.
2. Pump calls `take_command(now)` once, copies/moves it into reserved command
   capacity, preserving its absolute timeout budget. Fresh `advance_time(now)`
   continues even during flash. Match entire header/kind/length/data for duplicate
   commands; replay retained completion without hardware admission. Old epoch,
   wrap/exhaustion or mismatched key fences; preserve actual outstanding work.
3. Read: 1..256 bytes checked within one slot, healthy uncached data plus error
   status; no read during busy bank. Program: exactly G bytes, offset multipleG,
   within planned body or marker, payload from owned array; no runtime raw address.
   Erase: offset0/len0 as actual portable service uses; map to exactly E bytes,
   never infer size from len0. Permit command likewise offset0/len0. Read/program
   use checked addition and reject zero/overflow/cross-slot/cross-bank intervals.
4. QualifyErase scans the **entire inactive slot** and checks unchanged selected
   current, authority recovery status and portable Q. Q1 marker has1 OR Q2 any
   data/complement pair has11 OR Q3 all pairs complementary, markerzero, exact
   padding/tail and subordinate Valid/Corrupt. Any ECC, Unsupported, ambiguity,
   selected discrepancy or unresolved quarantine denies writes even if Q true.
   Unsafe00-pair/damaged-tail states cannot be "made safe" by invalidation/program.
5. After Q, durable authority debits/grants; backend consumes receipt durably and
   owns pending intent before erase. Obtain hardware semaphore and read back
   reviewed masks/translated address; inability to meet any predicate completes
   known failure without execute. No MMIO mutation occurs in this design job.
6. For each hardware command: all banks READ/idle, capture/clear old diagnostics,
   configure every used register and exact protected ranges, DSB/ISB, execute once,
   DSB/ISB, bounded poll/IRQ completion. Never issue a second operation while one
   is uncertain. CMDDONE=1, INPROGRESS=0, CMDPASS=1 and no failure flags plus
   settled read modes are necessary. Restore protection/NOOP and verify before
   release. Copy result and wake service; retain until acknowledged, with full
   release/acquire publication ordering or a bounded interrupt-safe fixed slot.
7. Erase -> complete healthy FF scan -> body programs -> exact healthy body,
   marker-erased and tail scan -> marker once -> complete healthy committed scan.
   For G256, sixteen16B subcommands per Program, with fresh status/masks/data for
   each; bounded total deadline covers all16. The entire G operation remains
   outstanding until all finish. No automatic retries on failure or lost completion.
8. Backend Ok is settled operation evidence, never portable Durable by itself.
   Only exact post-marker full scan and schema/CRC/complement/sequence match give
   Durable. Release only after `ownership_settled`, broker terminal capture and
   every published quiesce request has completed. Stale/late completions cannot
   advance another command or turn an unresolved diagnostic into a released slot.

## SRAM island and bank-access closure

Adopt bank0-only normal execution plus an SRAM0–2 flash execution island. During
bank1 operations neither CPU, DMA, debugger memory window nor other bus master may
read bank1. No application transport command maps arbitrary memory. A maintenance
permit freezes nonessential data/telemetry accesses and disallows clock/sleep
changes while allowing the reviewed UART/time servicing needed for liveness.

Place in SRAM before allocation freeze: command issue/poll/status/clear-status,
barriers/cache synchronization, bounded clock sampling, error capture, quiesce,
protection restoration, semaphore release and fatal park path. Include all
transitive callees, compiler memcpy/memset/divide helpers, veneers/trampolines,
jump tables, literal pools, vtables, panic paths and constants; fixed command,
completion, diagnostics and program data; stack and all referenced mutable state.
No logging/formatting, flash-backed panic strings or hidden allocator paths.
Linker load image stays bank0. Copy and compare/hash initialized RAM island before
using it; execute barriers and establish instruction coherency. Verify linked
relocations/disassembly, not just section attributes. No SRAM3 use/configuration.

Copy a suitably VTOR-aligned complete vector table into SRAM with validated
entries. NMI/HardFault and flash/ECC handlers and their complete dependency closures
reside in SRAM. SysTick and approved IRQs may stay in bank0 only with proved bank1
isolation; preferably place time/flash-critical ISR dependencies in the RAM island
and account their task-wake paths. Vector RAM alone does not relocate handlers.
Every enabled IRQ, tail chain, exception, return PC, DMA descriptor and speculative
fetch must be audited. Return to bank0 during busy may be allowed by the other-bank
rule, but never into target bank or a thunk/data dependency there. If unavailable,
remain safely in bounded RAM control/quarantine rather than return through flash.

Do not mask interrupts for the whole erase/program. Existing100us SysTick cannot
reconstruct lost wraps; a1s erase would destroy the absolute deadline model. Only
short measured bounded critical sections; preserve the time ISR and fixed wake
capacity. Preferred nonblocking RAM launch/poll progression permits the bank0
executor to advance_time and service UART while bank1 busy. No await in portable
HSM actions. If this closure cannot be proven, a new reviewed monotonic hardware
counter/time bridge and SRAM service loop are required; a fixed loop count and
existing RegisterIo::wait are not a deadline clock.

WWDT policy is fixed at startup under board review: use existing qualified RUN
configuration/window, schedule feeding from reviewed SRAM path only while healthy
bounded progress is demonstrated; never feed forever to hide hung flash. No
on-the-fly disable/reconfigure or guessed reset safety. If watchdog must reset
before a proved maximum/quiesce interval, refuse admission. Timeout does not mean
abort; no documented safe abort is assumed. Reset/BOR during operation may leave
partial media, so pending authority intent remains quarantined after any reset.

## Uncertainty, reset and allocation

Backend timeout reports Failed{quiescent:false} only as diagnostic, retaining exact
header/capture/completion ownership. One QuiesceRequest asks for bounded idle and
completion drain; it never writes reset registers to "cancel" flash. If DONE/idle
cannot be established by the quiesce deadline, stay fenced/Indeterminate with no
more media accesses or writes, fixed diagnostic storage and external recovery.
Other-bank execution being possible does not prove pump idle. Do not release
semaphore or start new lease while an old owner may still execute. If qualified
idle+drain is later proven, allow only the portable finite reconciliation allowance
(one post-marker full scan), keep quarantine, never silently retry a save.

Logical Reset preserves the handle, partial command, pending quiesce, retained
terminal, counters, fixed slots and quarantine. Advance epochs only after drain
and explicit settled handshake; never reacquire peripherals. MCU reset loses RAM
identity/results, triggers boot qualification and recovery scan; it does not refund
wear or automatically clear authority intent. No exactly-once save claim across
power cycles. Network failure after actual commit still permits old/new boot
recovery, with uncertain write state explicitly retained by authority.

All buffers, tasks, vectors, SRAM copies, mailboxes, tracing and authority transport
state are constructed before freeze. Zero alloc/alloc_zeroed/realloc/dealloc in
first poll, clear/reset/error paths, saturation, Drop and terminal release. Use
fixed arrays/enums; no Box/Vec/String/Arc, arbitrary Waker clone/drop, lazy task or
formatted panic. Concrete sizeof/map/stack and all-four allocator tests on the
real selected runtime remain T36/full-image obligations.

## Resource and timing budget (estimates, not measured fit)

Portable core+adapter+slots accepted ceiling8192B; retained review measured7960B
for its fixture. Proposed P adapter5120B/ceiling6144B is separate; no P activation.
Reserve additional driver static RAM2048B (two <=384B command/completion envelopes,
64B quiesce,256B authority exchange,256B diagnostics,512B trace,192B margin).
RAM code/literals8192B, vectors2048B, separate audited worst-case stack4096B:
16384B additional ceiling, total30720B including8192+6144. This leaves67584B of
96KiB for runtime/other stacks/application, not a proof those consumers fit.
Exact header ABI, closure size and maximum nested stack are unmet measurements.

E2048 boot=16 reads of<=256B. Normal save reads=32, erase=1, permit=1; G256=4
portable programs(64 hardware words), G16=41 programs. Bounds:
Tsave <= Tpermit + Terase + 32*Tread + (M/G+1)*Tprogram + Tscheduling,
with Tprogram256 covering16 clears/setups/programs/syncs and timeout margins.
Reconciliation adds <=Tquiesce+8*Tread. Permit timeout includes external durable
storage/round trip, not a fabricated MCU flash constant.64*40us=2.56ms nominal
program work only at datasheet conditions;41*40us=1.64ms alternative. Published
word time is typical, so neither is a max at32MHz. Establish every finite bound
and common monotonic domain under load; otherwise BackendConfig has no qualified
value and save stays disabled. Sustained UART/LED timing during erase is measured
in T18; no latency promise is borrowed from software tests.
