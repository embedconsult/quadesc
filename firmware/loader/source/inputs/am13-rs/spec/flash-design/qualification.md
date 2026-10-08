# Future fixed-operation acceptance matrix (data only)

This is a review checklist, not an executable Jetson job, HIL publication or
permission to run a device. Every row requires independent review of exact source,
image/driver/runner/procedure hashes, target identity, serialized lock from preflight
through recovery, finite deadline, raw observations and declared expected result.
T05 physical acceptance and T04 native/installation/client/recovery gates remain.
No SSH, devices, motor/CAN, supply switching, NONMAIN/security/fuses or authority
changes occur in this server job. No full T16/T18 acceptance follows host probes.

## Support predicates

| ID | Required evidence | Current disposition / safe action |
|---|---|---|
| F-ID | Confirm marked part/grade/revision, factory size512/banks2, UID, active bank mapping, existing protections and full MAIN identity | Document geometry supported, actual silicon unknown; deny writes |
| F-GEOM | Resolve sector2KiB, mask grouping/address translation, full16B ECC word path, <=16 full programs/line legality | SDK/current datasheet agreement, TRM conflicts retained; vendor/device qualification pending |
| F-MODEL | Healthy returned-byte erase0->1/program1->0 subset behavior and isolation <=G, unchanged other slot/finished granules, stable prestate interval | Neither docs nor finite host model establish power-collapse guarantees; conditional G256 only, otherwise unsupported |
| F-ECC | Healthy erased read, SEC quarantine, DED read error instead of boot loop, cache coherence, metadata attribution and ECC behavior on torn words | Explicit unqualified error return/erased/torn path; no FF fabrication |
| F-POWER | MCU rail measured within3.1..3.6V during work, temperature/grade, clock/pump/reset applicability | ONLY3.3V debug power user report; no measured qualification |
| F-RAM | Exact final ELF disassembly/relocations and every vector/ISR/call/data/fault closure; stack/no bank1 fetch; no SRAM3 | Existing fixtures fit, island/full image not implemented |
| F-TIME | Resolve the FRI conflict: SPRUJF2B §13.5.2 p532 zero-wait note versus SPRSPC3A §6.8.3 Table6-1 p98 RWAIT>=1 at 0<CPUCLK<=100MHz (RWAIT=0 unsupported); select RWAIT only after authoritative applicability, at least the datasheet minimum meanwhile. Read back actual FRDCNTL/cache controls; if FRI registers change, validate TI-B §13.5.6 RAM procedure with no covered flash access and nine-cycle propagation before return. Establish finite read/permit/program/erase/clear/cache/quiesce bounds at32MHz and selected wear/voltage/temp; IRQ/UART/LED/watchdog schedulability | Current clock.rs neither sets nor checks FRDCNTL; wait-state choice and FRI procedure unqualified. Typical word timing is not a max; no deployable BackendTiming |
| F-WEAR | Per-slot and total pump history/derating, native durable authority/session/anti-replay/crash tests, persistent quarantine | Interface/design only, allowance0 until qualified |
| F-INTEGRATION | Reviewed T06/T36 real runtime, fixed resources/allocator tests, T17 I5/I6/profile/client integration and full linker reservation | Pending; no generated files/endpoint activation |

All predicates required for writing. The host probe proves their conjunction fails
closed when each is absent; it does not create evidence for any of them. Read-only
boot itself requires F-ID/F-ECC/idle readiness; cannot assume even fallback reads
are safe until qualified.

## Fixed campaign rows

| Row | Fixed operation and prerequisites | Pass oracle and retained evidence |
|---|---|---|
| H00 host | Validate layout+all G geometry, overflow/address/length guards, reject non-MAIN and malformed image inputs | Host report includes positives/negative controls, exact sources; no flash qualification |
| N00 native no-device | Build/qualify exact reviewed T04 runner/client/authority source on Jetson ARM64 under existing authority, fixed fake fixtures | Native identities, actual storage crash/commit/replay/ledger loss tests; no device step until independent acceptance |
| R00 read-only target | Under reviewed serialized read-only procedure identify chip/bank/protection, copy full512KiB MAIN, validate vectors/code/calibration/guards and external backup hashes | Actual extent/UID/revision/protections and complete backup known.49KiB readback is insufficient; no alternate address probing beyond allowlist |
| R01 linked admission | Compare exact candidate ELF/BIN/hash, LMA/VMA sections and RAM closure report to bank0 bound and reserved slot/guards | Reject any image load bytes in bank1/calibration/NONMAIN; readback matches installed bank0 and reservation policy |
| W00 scratch qualification | After F predicates/review, one exact inactive reserved sector erase plus finite approved16B/G program pattern and readback sequence; charge all erases | Healthy FF/data/ECC result, post-op idle/protect; before/after full non-target hash plus both16KiB guard regions unchanged. Never use selected record or arbitrary address |
| W01 normal save/reboot | Fixed schema1 period/duty within100..10000/0..1000, capture revision, XCP save then Resolve/ReadDurable, controlled reset/reconnect | Request receipt differs from durable success; full stored metadata+bytes restored, active revision0 after boot and historical saved labels; second save alternates; equal exact snapshot DurableExisting causes no erase |
| W02 logical lifecycle | Stop/Start/Reset at each accepted/pending/retained state, dropped replies and slow client | No handle reacquire, new lease, refund or premature release; single command, drained epoch change; RAM snapshot unchanged; no post-freeze allocator attempts |
| W03 MCU reset cuts | Fixed finite offsets at erase admission/active/end, each body G boundary plus representative within-word cuts, marker start/middle/end, before/after completion response | Exact old/new or documented defaults; Q and authority quarantine enforced, no blind reissue. Capture trigger timing uncertainty, waveform, status, both full slots and non-target guard hashes. Reset does not equal supply removal |
| W04 physical power cuts | Only after separately proven controllable power fixture/procedure and F-POWER; finite offsets as W03 with actual measured rail waveform | Qualify admitted power-collapse model for tested conditions; report untested cuts/conditions. Current fixture has no proven switching here; row explicitly unperformed |
| W05 corruption/ECC | Dedicated reviewed sacrificial-state fixture within allowed inactive region; fixed healthy, pair11, unsafe00, damaged tail, SEC/DED and torn ECC cases; each destructive creation accounted | No programmed corrupt active image or software ECC suppression; error surfaced/quarantined, Q rejects unsafe before wear/erase, remaining slot recoverable. Real ECC injection technique itself requires review; arbitrary extra same-word programming is forbidden |
| W06 fault/time | Exact fault windows: lost completion, persistent busy, sem denial, known protection denial, authority loss, clock timeout, completion saturation, stale epochs | No new write, no timeout-as-idle, bounded quiesce and one reconciliation scan, durable quarantine across reset; MCU/UART liveness and watchdog bounds measured |
| W07 wear/session | Authority crash before/after debit, consume, issue, settle; host restart, MCU reset, old packets, ledger absent/torn/rollback, exhausted budget | No duplicate physical erase, no receipt/lease replay, no refund, total pump accounting; unavailable authority denies saves while qualified offline boot restores committed settings |
| B00 MAIN BSL recovery | After separate reviewed release, serialized MAIN-only recovery using exact pinned diagnostic/recovery image and full known backup; reserve/ledger handling explicitly approved | Correct bounded ranges and image hash/readback/identify/UART response, fixture restored or quarantined. Never factory reset/mass erase including NONMAIN, no supply/security/fuse changes |
| E00 end of demo | Normal reviewed save of documented defaults and confirmed reboot restoration, or documented read-only/quarantine if unable | No destructive "cleanup" erase, budgets retained, exact final MAIN/slot hashes and fixture disposition. Retain known image/procedure for repeatable recovery |

Whole MAIN/guard hashes exclude only the exact admitted inactive E sector for
W rows. Snapshot before/after and independent readback must establish that nothing
else changed; ECC correction status is recorded alongside bytes/hashes. Before
first mutation require independent known recovery image and backup of **all**
MAIN; bank0 recovery installation must preserve reserved bank1 or be classified
as reprovisioning. An unexpected hash, DED, reset loop, lost authority, deadline
or uncertain operation stops the campaign under the same T04 lock. No retry/
automatic reflash/power cycle; retain state and request reviewed recovery disposition.

Finite directed faults/measurements can demonstrate implementations and tested
conditions, not prove all physical cuts or datasheet-unspecified monotonicity.
Independent review must either obtain sufficient chip guarantees for R1 or refuse
this portable backend. No CRC probability argument replaces Q/complement theorem.
