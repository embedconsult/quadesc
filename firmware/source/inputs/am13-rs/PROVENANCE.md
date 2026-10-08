# Source provenance and notices

The register/startup algorithms and board map derive from source revision
`802aacd360cb0237842ff9f18dfdac312318137e`, with independently captured input
revision `e9c87dc2ed34ee16cf7a8e0c539cb093e35d37ed`. `provenance.json` retains
inspected input identities and SHA-256 values. Current component revisions,
imported modifications/untracked files and editable snapshot hashes are in
[firmware source provenance](../../../source-provenance.json); a base commit alone does not represent
those imported changes. Original workspace snapshots remain outside the handoff.

No explicit license grant was found in the inspected original bring-up source;
existing source/reference notices are retained and no license is inferred.
TI SDK material supplies register documentation: vendor C source and SDK linkage
are not relabeled as authored Rust. Included pin CSV/SysConfig files retain their
original input hashes.

Primary reference review used TI AM13E230x TRM SPRUJF2B, SYSOSC/MCLKDIV2 section
3.4.1, SysTick section4.4.2, GPIO/pad chapters14/17 and UC/UART chapters28/29;
SDK26.01.00.03 register definitions provide the matching register details.
The public [TI TRM](https://www.ti.com/lit/ug/sprujf2b/sprujf2b.pdf) identifies
the publisher source; provenance records preserve the inspected revision hashes.

Embassy time-driver0.2.2, executor0.9.1 and time0.5.1 were reviewed through
unmodified Cargo source and package checksums. Locks pin the exact dependency
resolution. Framework/platform ownership and the application's active programming,
calibration, recovery and physical evidence are described in the
[engineering manual](../../../../documentation/html/engineering/index.html).
