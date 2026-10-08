# Primary evidence and unresolved device predicates

The publisher references below are identified by inspected revision and SHA-256.
The source provenance inventory retains input digests independently of private
workspace locations. TI sources carry their original notices; SDK register
documentation is separate from the authored product code.

| ID | Exact authority and inspected locations | Result and limit |
|---|---|---|
| TI-B | Retained SPRUJF2B PDF SHA31aa9d5365f2d892c76b08977001b2653712a1ebf4046c1e9e6d75639f62e53c; retained trm.txt SHAe88d28157551aec1628a57ad3f50e51616e825b7a5a605224bb3d493cbaa1db9; §§13.1–13.5, 3.7.2,12.2.2 | Bank exclusivity, command flow, ECC and cache rules; contains internal geometry/protection inconsistencies. |
| TI-old | Retained February SPRSPC3 datasheet.txt SHA174069d7171283eb95eaa455544abce37c6200307e18943ef358f47948003241 | Historical reference; incomplete electrical flash limits. Never infer full MAIN from diagnostic read length. |
| TI-A | [SPRSPC3A, February 2026 revised August 2026](https://www.ti.com/lit/ds/sprspc3a/sprspc3a.pdf), SHA3643b949b6e8188a67a706b2c88ce5f4d4c734bb7278821495d55c7f574efa98; §6.8.3 Table 6-1 p98, §6.8.3.1 pp99–100; device comparison/memory map | Current downloaded primary reference, separate from preserved old copy. Table 6-1 requires RWAIT>=1 at 0<CPUCLK<=100MHz and footnote excludes RWAIT=0. 512 KiB part, 2 KiB sectors, 128 data +16 ECC bits. Advance Information status retained. |
| TI-E | [SLAZ778](https://www.ti.com/lit/pdf/slaz778), SHA1a4f8c5280396714d3cbbde0dd43d29056f31defb305a2dfb56c2e5aab398816 | Revision1.0 errata, only listed advisory concerns LFOSC/STANDBY. Absence of a flash advisory is not interrupted-write proof. No STOP/STANDBY selected. |
| SDK | Retained AM13E230x SDK26.01.00.03: source/driverlib/am13e230x/dl_flash{,ctl}.{c,h}, device hw_nvmnw.h/hw_factoryregion.h, flash/main.c and default GCC/TI linkers | E=2048, bank=0x40000, sector address >>11. RAMFUNC execution helper, barriers, semaphore and ECC word APIs. Source behavior is not a silicon guarantee. |

The live TI product page links Rev B TRM and Rev A datasheet. Browser retrieval of
TRM failed; retained Rev B was inspected directly, not silently replaced with
search's older Rev A. The owned downloaded datasheet and errata are preserved in
full locally for review. Public sources are read only; no forum posting/contact.

## Geometry and legal programming

AM13E23019 MAIN is documented as [0,0x80000), bank0 [0,0x40000), bank1
[0x40000,0x80000). SDK linker agrees. 128 sectors/bank at E=2048. Factory readonly
SRAMFLASH at0x60111074: bits11:0 size in KiB, bits13:12 bank count; SDK also has
ROM_SRAMFLASH at0x6011101c. Size512 and count2 are necessary predicates, not proof
of part/revision. Pin verified device marking/UID/TRACEID, revision and exact
factory fields under T04 before accepting a profile. Older datasheet PARTID fields
contain TBD and suspect offsets; do not invent a numeric part-ID allowlist.
Bank swap must be verified inactive with qualified register interpretation;
physical slot identity is not just a CPU virtual address. No bank-swap write.

TRM Table13-1 gives 16-byte word, 256-byte word line, 2048-byte sector; §§13.1.1,
13.3.5 and example diagrams also contain 1 KiB claims. Current datasheet and SDK
agree on 2 KiB; design selects that interpretation, with target confirmation still
required. §13.3.4.2 gives 8-byte single-word alignment while §13.3.4.3 says128-bit.
SDK wrapper enforces16-byte alignment; comments and partial 64-bit APIs differ.
Use only 16-byte-aligned full 128-bit programming with both ECC bytes generated.
No byte/subword update, ECC override, repeated program, bank erase, mass erase,
factory reset, address-translation override or NONMAIN command is exposed.

SDK hw_nvmnw.h splits ECC into two bytes for data63:0 and127:64. This does not
prove that a torn operation disturbs only8 or16 bytes. One-shot granules and
commit isolation are mandatory. TRM refers the maximum programs/word-line to the
datasheet; the inspected datasheet does not supply a numeric subword pulse limit.
The candidate performs at most16 full-word programs/256-byte line, each word once.
Get an authoritative limit/applicability before allowing this sequence; successful
example execution cannot substitute. SDK flash example erases an entire inactive
bank and tests2048/100/7 bytes; its padding and partial-ECC behavior is unsuitable
for this contract and is not adopted.

## Controller, protection and synchronization

Separate NVMNW command registers (SDK hw_memmap.h base0x40042000) from
FLASH read-interface controls (0x40028000). Do not reuse the latter base for
command offsets. hw_nvmnw.h fixes register offsets; the map and SDK are hashed. TI-B
§13.3: boot ROM/BSL may leave nondefault controller configuration. Establish actual
idle before configuration/read. Inspect CMDDONE, CMDPASS, CMDINPROGRESS and every
FAILWEPROT/FAILVERIFY/FAILILLADDR/FAILMODE/FAILINVDATA/FAILMISC diagnostic. DONE
alone is not success. All banks must be in READ mode to start program/erase.
Reinitialize every relevant command field; clear old status via the documented
clear-status command with its own bounded completion; default COMMAND=NOOP when
idle. Preserve diagnostics before clearing. Set ordinary translated MAIN address,
ONE_WORD for program, SECTOR for erase. No unsupported size combination.

SDK executeCommand places DSB/ISB before execute and before status polling and
is RAMFUNC; its polling has no deadline. It is a reference sequence, not a usable
bounded backend. SDK semaphore acquisition is one attempt at GSC FPC_FLSEMREQ
and checks ASSIGNED+MATCH; release uses FPC_FLSEMCLR. A future driver must acquire
under existing privilege, never change GSC permission/security configuration to
make it succeed. Busy/denied means refusal, no retry loop/forced takeover.

Dynamic write masks reset protected after operations. Static write/erase and hide
configuration must already permit the reserved regions; no BCR/security changes.
SDK sector masking uses local sector<32 in CMDWEPROTA, otherwise bit
(local_sector/8)-4 of CMDWEPROTB. For proposed A/B this predicts bits10/11, each
covering16 KiB. TRM §13.4 still claims1/8 KiB protection units. Exact mask/bank
selection/readback coverage must be reconciled against silicon; do not trust an
unreviewed shift to protect code. Keep all other groups protected, preserve
NONMAIN protection, restore/read back protection on completion. If mapping would
unprotect executable memory or both slots contrary to reviewed guard policy, deny.
Controller address checks remain mandatory even with hardware protection.

## ECC and read coherency

Corrected MAIN reads, SEC IRQ and sticky SYSCTL FLASHSEC; DED NMI or SYSRST
controlled by FLASHECCRSTDIS (TI-B §3.7.2). EAM FRI_SEC/DED capture flag/address/
master; address is meaningful only with its flag set (§12.2.2). Capture status
before clearing, take exclusive ownership of error attribution, clear only after
recording previous faults. Any preexisting/unattributable error denies healthy
read; code-fetch SEC is not silently assigned to a data slot. A SEC-corrected value
is never healthy. DED payload is never accepted or fabricated as FF. Report
ReadIssue; persist quarantine. No enabling error suppression or ECC bypass.

Reads must flush/invalidate the documented read-interface/CPU caches and buffer
state, with qualified barriers, before checking programmed data. Corrected loads
alone and READVERIFY alone do not prove ECC cleanliness. No programming of FF to
"fix" an erased marker. Establish whether erased data+ECC can be read healthily,
and whether torn-word ECC causes a recoverable error or reset/lockup. A qualified
SRAM NMI/HardFault read guard may report errors if return semantics are proven;
unproved recovery means the backend cannot offer R1 full-slot reads. Never
blindly retry a faulting load or clear reset status and declare Empty. A reset-only
DED path can defeat fallback by looping on the bad slot; it is an explicit support
gap, not an acceptable implementation of Unreadable. Raw ECC aperture0x60200000
is diagnostic-only; no arbitrary/raw alias write or assumption that bypassed reads
preserve the portable model. Conditional monotonicity must apply to returned
healthy bytes, including all effects of ECC, not only physical cell bits.

## Electrical and timing evidence

TI-A §6.8.3.1 lists program/erase VDD3.1..3.6V and up to10mA additional current.
The SoM's ONLY3.3V debug power is user-reported, not measured. Retain existing
supplies; require measured rail envelope/droop at the MCU, current headroom,
reset/BOR behavior and applicable temperature/qualification grade. No supply or
BOR/security setting changes are authorized here. Voltage-collapse isolation and
monotonicity are not guaranteed by the normal-operation voltage table.

TI-A: minimum endurance20k cycles both per write/erase and entire-device pump;
retention20 years at -40<=Tj<=85C. Unknown historical use consumes unknown budget.
40us word and5.1ms full-sector programming are TYP at maximum device frequency,
not maxima at nominal32MHz. Sector erase TYP/MAX(ms): <25cycles15/55,
1000cycles25/130,2000cycles30/221,20kcycles120/1003. Do not extrapolate/interpolate
an unqualified deadline or multiply40us into a worst-case guarantee. FCLK max50MHz.
SPRUJF2B (March 2026 revised August 2026) §13.5.2, printed p532
(retained trm.txt lines22395–22416), says RWAIT defaults to2, gives
RWAIT=ceil(MCLK/FCLK)-1 and notes zero-wait reads only at MCLK<=50MHz; it also
directs supported RWAIT values versus CPU clock to the device datasheet.
SPRSPC3A (February 2026 revised August 2026) §6.8.3 Table6-1, printed p98
(retained sprspc3a.txt lines6021–6044), instead requires FRDCNTL[RWAIT]>=1 for
0<CPUCLK<=100MHz and explicitly says RWAIT=0 is unsupported. These primary
statements conflict for the proposed32MHz clock. Do not treat RWAIT=0 or the
existing clock setup as a qualified32MHz read-interface setting: current clock.rs
neither sets nor checks FRDCNTL. Resolve authoritative applicability before final
RWAIT selection; until then constrain any proposed setting to the datasheet minimum
without claiming silicon qualification. Later FRI/F-TIME validation must read back
actual FRDCNTL and cache controls and, if changing FRI registers, use TI-B §13.5.6's
RAM configuration procedure: no covered flash access and nine-cycle propagation
before return. No PLL/clock switching is authorized to match a table. Need verified
pump clock source and program/clear-status/read/quiesce worst cases for this
clock/grade before BackendTiming is instantiated.
The1.003s erase figure is planning evidence, not an overall save deadline.
