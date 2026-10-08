# T16 physical flash design candidate r1

Status: prerequisite design/evidence only; independent review required. No flash
implementation, linker modification, target image, native qualification or release
is supplied. T05 physical acceptance and serialized T04/Jetson qualification remain
mandatory. T17 portable I1–I4 remain unchanged. This candidate enables review of
what can be built and identifies where the accepted portable model cannot yet be
asserted for this silicon. Do not activate persistent saves from these documents.

Read [primary evidence](primary-evidence.md), [placement](placement.md),
[driver interface](driver-contract.md), [wear authority](wear-authority.md) and
[qualification matrix](qualification.md) together. `layout.json` is design data,
not a deployable job. Geometry admission is arithmetic design evidence, not a qualified flash backend model.

Candidate: AM13E23019, unswapped 512 KiB MAIN, two 256 KiB banks, 2 KiB sector
E. Candidate portable G=256 isolates the commit into a separate documented word
line; it is conditional on a qualified <=256-byte disturbance scope, monotone
partial operations and reported ECC behavior. Hardware word programming is 16
bytes with hardware ECC; a 256-byte portable operation would execute sixteen
one-shot word commands. G=16 is a smaller alternative only after proving <=16-byte
disturbance/isolation. Neither geometry is target-qualified. If disturbance exceeds
256 bytes or ECC silently substitutes a different accepted codeword, reject this
portable backend; do not widen the contract or hide the behavior in a mock.

MAIN reservation proposal: bank0-only firmware and RAM load images; A at 0x78000,
B at 0x7c000, each 0x800 bytes, with the final 32 KiB reserved as two 16 KiB
protection/guard regions. Capacity and current image separation are proved by the
host probe. Exact target identity, protection mapping, electrical conditions,
program-operation word-line limit, deadline maxima at 32 MHz, ECC/error recovery,
interruption behavior and durable wear service remain explicit release predicates.

Current source revisions, imported changes and per-file hashes are recorded in
`firmware/source-provenance.json`. These generic portable-backend design notes
retain their conditional assumptions. The drone application's concrete
calibration save/recovery path and loader are documented separately in the
customer engineering manual. Existing notices are preserved.
