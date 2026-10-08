# Primary evidence and reference experiment

Evidence is source-specific. This is not an ASAM compliance statement. Public third-party source revisions and hashes are retained in
[source identities](../../evidence/profile-p-open-v2/sources.json); publisher URLs
identify the original material.

## Exact source/configuration inventory

Vector XCPlite commit `88e3cbeec79318f50067eede2502bcf4d34dab5f`, upstream tree
`fcf57f8dc3b22f4db1afebf879e14b91a880e480`, is retrieved as a complete commit archive.
Seven earlier archived Vector files match byte-for-byte. No upstream source or
independent client is patched. The separate `reference.c` experiment fixture is a deliberately separate
application fixture: two4-byte calibration objects plus Vector's EPK segment,
loopback127.0.0.1 UDP, local persistence, WRITE_ONCE + FINALIZE_ON_CONNECT A2L.
This is not a renamed upstream example or a prototype P endpoint.

[Configuration](https://github.com/vectorgrp/XCPlite/blob/88e3cbeec79318f50067eede2502bcf4d34dab5f/src/xcplib_cfg.h)
lines85–118 enables segments(count8), persistence and EPK; single-page/start-on-
reference/freeze-on-disconnect options are not enabled. `OPTION_CANAPE_24` is set.
[xcp_cfg.h](https://github.com/vectorgrp/XCPlite/blob/88e3cbeec79318f50067eede2502bcf4d34dab5f/src/xcp_cfg.h)
lines296–335 derives page/copy commands unless SINGLE_PAGE; persistence derives
FREEZE. The CANape copy-all workaround is disabled by OPTION_CANAPE_24. The observations below distinguish default and single-page build configurations.

All shipped overrides were inventoried: `no_a2l` disables persistence/A2L and uses
absolute segment addresses; `ptp` adds socket timestamps; `shm` selects shared-memory
mode; `rtos` disables filesystem persistence and reduces resources; `raw` selects
raw Ethernet transport. None is an executed one-page save/restore precedent here.
Only default and the supported `XCPLITE_CFG_OVERRIDE=single_page.h` (one define)
were built. No RTOS fetch, raw socket, PTP, SHM, install or Rust tool target ran.

[a2l_writer.c](https://github.com/vectorgrp/XCPlite/blob/88e3cbeec79318f50067eede2502bcf4d34dab5f/src/a2l_writer.c)
lines60–83 emits default two-page FLASH segments with working page0 properties3F
and read-only page1 properties0F; single-page emits RAM/one-page with ECU don't-care
but XCP read/write WITH_ECU_ONLY. Its command list follows compile flags (111–125).
There is no emitted INIT_SEGMENT field in those page declarations. The runtime
GET_PAGE_INFO nevertheless returns3F and init1 in both builds; GET_SEGMENT_INFO
returns MAX_PAGES2 in both. Thus the single-page A2L count and access properties
disagree with runtime. GET_CAL_PAGE/SET_CAL_PAGE/COPY_CAL_PAGE disappear in that
build, while FREEZE remains. Default runtime page1 succeeds; single page1 returns
FE26. This closes the inventory question; it **does not qualify** an internally
consistent upstream one-page model or justify copying its INIT_SEGMENT assignment.

Runtime init1 is assigned from a page constant in `cal.c:980`, even for segment2,
whose initial data are unrelated to segment1's LED values. The experiment therefore
cannot establish the field's intended standards meaning. P selects its own explicit
boot-only self-reference policy with that limit disclosed.

## Host-only execution and raw observations

The two reference builds used GCC15.2.0/CMake, Debug and parallelism2 on x86_64 Linux.
They are source-specific host experiments; no reference binary or internal build
log is included as a customer application.
`probe_reference.py` sends real UDP XCP packets to the fixture, records every full
Ethernet transport frame and raw CTO, and preserves whole persistence files/A2L.
Independent `audit_reference.py` checks those artifacts without importing the probe.

Final campaigns: default58 exchanges/four process starts; single49/four starts.
Observed initial, selected, cleared, re-enabled and divergent persistence states:

* No selected segment: F9 returns FF, file unchanged despite RAM edits. P deliberately
  rejects freeze0 rather than copying that vacuous success.
* Fresh A2L/file, LED selected: F9 returns FF, but bytes appear at1056..1059 rather
  than LED payload800..803. First restart reads defaults1000/500. `writeCalseg`
  records the data position (`persistence.c:194`); Freeze adds sizeof(tBinCalSeg)=256
  again (`:348`). Load instead records the descriptor position (`:459–468`). This
  explains the observed fresh-file offset discrepancy; no reference patch is made.
* After load, clearing FREEZE leaves the file unchanged. Re-enabling saves LED
  4000/600 at800..803; next restart restores it. Unselected control remains2000/250.
  This proves this loaded-file path, not the fresh-file path or power-fail durability.
* Default build: after that restart, XCP working page is edited to5000/100; ECU is
  explicitly page1 (loaded4000/600), XCP remains page0. FREEZE selection is confirmed
  before F9. Save preserves4000/600 and restart reads4000/600. This characterizes
  saving the ECU-accessed page, not universally saving the XCP page. P avoids this
  divergence by having one page/owner and explicit captured C.
* A local directory substituted for the persistence filename produces FE24 before
  any positive F9 response. It is a **synchronous** open failure, not accepted-then-
  failed evidence. Subsequent GET_STATUS is not evidence for asynchronous failure.
* Reference reserved response bytes retain prior bytes (e.g. GET_SEGMENT_MODE's
  reserved byte). Raw evidence is preserved; semantic assertions mask only documented
  reserved positions. P instead explicitly returns zero reserved bytes.

## Public research and its boundary

[ASAM's public XCP overview](https://www.asam.net/standards/detail/mcd-1-xcp/wiki/)
separates logical memory segments, implementation-dependent page switching and
startup initialization. It does not define the needed INIT_SEGMENT edge case or
late-failure rule. It supports separating physical record storage from logical
calibration addressing; that is an inference, not a one-page conformance proof.

[NI manual371601F](https://download.ni.com/support/manuals/371601f.pdf), printed
5-147/148 and6-108/109, describes pending STORE_CAL bit0, clearing when fulfilled and
optional completion notification; 5-162/6-119 describes selecting FREEZE. It does not
settle bit disposition on post-FF failure, uncertain media or new connection. The
retrieved archived PDF is hash-identical to prior evidence. P's sticky-unfulfilled,
no-event, fenced-reconnect rules are explicit application choices based on these
limits. No negative event payload or later command error is inferred.

Unmodified pyXCP0.29.18@016cf3e44364e9cd93966d144a39d342578a0391 and
pya2ldb1.0.353@c19c3ad2f285d1230e334bad81eeb09cbaaa3031 source pins/hashes carry forward.
Their repeated codec/parser checks are scoped to the changed candidate metadata,
not a claim of endpoint execution. The retained source identities scope the reference observations and 43 original
wire pairs; no historical execution archive is delivered.
The separately added transition checker proves constraints of design fixtures only;
it is expressly **not** observation of asynchronous reference behavior.
