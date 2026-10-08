# Release tests required after independent design approval

These are endpoint/firmware obligations; reference execution and contract checks
in this design package do not satisfy them. Run only the applicable newly changed
checks; carry independently verified unchanged portable campaigns by source identity.

1. **Matched bundle and client:** independently pin the eventual endpoint/library/
   generated composition/image/A2L/transport/descriptor/policy hashes. Keep S0 IDs
   distinct; prove actual S0 F9 returns FE20 for valid and malformed F9. P client
   must reject S0, synthetic fixture identity, mismatched schema/profile and stale
   A2L before E6/F9. Use unchanged pyXCP0.29.18@016cf3e and
   pya2ldb1.0.353@c19c3ad; verify installed sources. Dedicated process sets public
   General.disable_error_handling=True and prevents another Master re-enabling it.
2. **C21 page/map/admission:** real SxI traffic checks every page/segment width,
   response reserved byte, getter and invalid length/mode/index/reserved precedence;
   unknown COPY, selected/deselected FREEZE, no unselected empty save, same-page
   idempotence, and one-page companion/A2L agreement. Preserve LED period u16 LE
   100..10000ms, duty u16 LE0..1000, defaults1000/500 and current S0 scalar behavior.
   No physical addresses or hidden writable pages. Known disarmed/lock/schema/rate/
   deadline refusals must preserve accepted history; fresh Q/permit failure occurs
   only after retained acceptance. Verify exactly one F9 request on timeout/FE10/FE24.
3. **C21 asynchronous save:** instrument separate receipt, staged capture, accepted
   C, FF, emitted/acknowledged backend commands and confirmed durable transition.
   Poll during deliberately delayed media; change RAM after C and show stored bytes
   equal C, not later RAM. Inject post-FF definite failure and uncertainty separately.
   Verify exact sticky bit, no event or second F9 response, one in-flight slot,
   disarmed/maintenance permit, no GPIO-output reservation for Save, and failure
   never rolls RAM back. Test fast DurableExisting (no required observed pending
   poll), current durable superseded by another client, retained key/C/reason on a
   rejected next save, and nonmatching late completion fencing.
4. **C22 transport/ownership fences:** lose FF, delay/reorder old replies and exact
   backend completions, timeout each stage; keep original key/C/backend custody.
   No status query on ambiguous stream; qualify real idle/drain/fresh-domain boundary
   before new CONNECT. Reconnect while backend remains uncertain must remain fenced.
   Once true idle and all completions settle, copy each of four broker results then
   retire/release and complete epoch handshakes; test saturation, exact +1 increments,
   exhaustion/no wrap, partial handshake and diagnostic-only reconnect after failure.
   On new connection, bit0/freeze0 plus retained failed predecessor must not be called
   success. No retry/cancel/reissue to satisfy a timeout. Prove late terminal refinement
   requires matching identity and does not free unsettled ownership.
5. **C23 readback and reboot:** exercise every chunk size1..7 over640-byte latch,
   intervening RAM/save/other-client completion, seeking/refreshed latches, read-only
   rejection and unchanged cursor on errors. Persist host key/C/result before another
   save. Confirm record labels and exact canonical bytes separately from current RAM.
   On MCU boot, initialize Owner once from validated R3 record/default before outputs
   or Ready, with normal revision0 and historical saved labels separate. Logical Reset
   must preserve live RAM; only actual reset/power cycle reloads storage. Corrupt,
   unsupported, interrupted and unreadable saves select prior/new valid values or
   explicit documented defaults under R3; never silent recovery success.
6. **Firmware/native/HIL:** I5 uses declarative Bloxide topology, pure guards and
   bounded synchronous actions; async I/O carries owned commands/completions outside
   actions. Generate from source TOML through reviewed codegen; never edit generated
   Rust. Move each resource once, retain across lifecycle Reset, freeze allocator
   before Start/admission and prove zero post-freeze allocation attempts including
   error/reconnect. T06/T36 gates remain. Measure full image RAM/stack/timing against
   declared ceilings, not just arithmetic. T16 supplies real AM13 MAIN G/E/M/Q/ECC,
   isolation, quiescence, voltage/timing, cache/RAM callgraph and durable wear authority.
   No inference from x86_64 files or portable cuts. Native ARM64 runner and MCU builds
   retain source/toolchain/target/hash/log identities. All future physical MAIN-only
   UART/reset/power-cut work goes through reviewed serialized T04/Jetson procedure,
   preserving BSL recovery; this job performs none and exports no HIL artifact.

No T34 independent Git consumer qualification, T06/T09 blocked service retry,
credential workaround, public framework publication, CAN/DAQ/motor feature work
or OS/bridge/security change is part of these design checks. The explicit later
integration plan does not authorize bypassing any prerequisite.
