# AM13 generated diagnostic composition

## Selected host simulated-media boot

The `uart-persistent` and `xcp-p` host adapters may name one strict raw fixture containing exactly two 1024-byte simulated slots. Startup rejects a missing or wrong-size fixture. It classifies both slots with the pinned portable classifier, recovers one selection/health policy, and constructs the generated actor's sole `LedController` from that selected configuration. The same `Recovery` initializes the attached persistence service before Start. The fixture is a serialization of the simulator's media after backend commands, not a second active settings store or an embedded persistence protocol. Its contents can be copied between host endpoint processes for a simulated restart. Corrupt, unsupported and interrupted bytes follow the reviewed classifier defaults and write policy; no physical recovery authorization is inferred. Without a fixture, selected host behavior starts with empty simulated media. Target boot remains defaults and target SAVE remains unavailable.

`examples/am13-xcp-control/system.toml` selects the existing `led::led-owner` actor and Embassy runtime. Platform construction consumes AM13 T05 peripherals once, configures inactive motor outputs, the dedicated active-low LED and UC4 at the current 19,200 8N1 diagnostic basis, and retains the sole SysTick. The output service exclusively consumes the split schedule slot and owns the LED; the UART service exclusively owns UC4, a 64-byte line buffer and a reply reader. Both services are spawned before `freeze`; they first poll only after the generated setup returns. The sole actor owns T07 state. Reset/Stop never reacquires peripherals.

Plain `GET`, `SET PERIOD <u16>`, `SET DUTY <u16>`, `SAVE`, `STATUS` translate to the existing Read/Apply/Release typed seam. A mutation uses one operation key, waits for a correlated retained terminal outcome, releases it and only then advances the sequence. A rejected pre-send leaves the key reusable; admitted but uncorrelated or timed-out requests close the session as uncertain. `SAVE` returns unavailable. Output acceptance is software scheduling, not optical proof.

The process-wide startup arena freezes irreversibly at generated `platform.freeze`, before actor first poll/Start, UART admission and SysTick interrupt enable. Every GlobalAlloc entry checks the flag, increments its own fixed counter on a forbidden attempt, and halts without formatting. High-water and counters are retained in statics. A cross-build proves only compilation/linkage and the fixed memory envelope; host generated-runtime child-process qualification must observe real first polling, messages, timer and cleanup. General resolver confinement, physical behavior, persistence, XCP and complete T36/T13 coverage remain unproved.

## AM13 correction: faults and transport bounds

The output consumer latches an installation or edge readback failure against the exact published schedule. Installation failure also occupies the completion reserved at take. Fault closes output admission immediately. The service then attempts inactive LED output and records verified readback or unknown physical state if that attempt fails. The software Applied result and revision remain unchanged. STATUS reports the bounded latch and allocator counters; readback is electrical evidence, never optical evidence. A same-value Apply does not reserve an output slot or change phase/revision.

After either fault, the LED service repeatedly polls the portable fault-retirement operation. It cancels any queued Applied schedule with its exact key only when completion capacity exists; full FIFO retains the queue until owner draining. It does not reopen admission or retry an installed schedule. Diagnostic STATUS reports queued, pending-terminal and last-drained exact keys and the separate installed-command fault. Coordinated fault acknowledgement and new-epoch resume require owner drain, verified inactive output and explicit control; the diagnostic itself does not request recovery. Unknown safe-off keeps recovery closed.

The generated Embassy host qualifier's installation-fault case moves an owned `esc-som-board::Led` backed by a fixed register model into its LED service, forces its pad readback to fail after the first changed Apply, and runs both failed installation and failed safe-off through that board API after freeze. A separate generated case holds A installed while B commits, injects A's later edge readback fault, and observes B's exact-key cancellation and retained software readback. Its four-operation allocator guard covers those calls and the generated owner actions. This remains a hosted time-driver and register-model observation, not a target IRQ or physical LED observation.

The UART service permits at most 400 ms per complete response write/flush, measured by the selected monotonic Embassy timebase. At 19,200 8N1, the 640-byte fixed response capacity needs about 333 ms on an uncongested wire; 400 ms exceeds that serialization budget. A timeout ends the session because some bytes may already have left UC4. When a mutation has been admitted and its response cannot be delivered, retain and report its exact key as uncertain; never translate it to Rejected. The owner result is released only after a complete response flush, then its fence is acknowledged before the next command. The owner wait remains five seconds (request expiry six seconds). Every synchronous RX error clears the partial line and awaits one millisecond, so repeated errors cannot monopolize the executor. A fixed counter saturates and is visible through STATUS when UART remains usable.
# UART flash SAVE source state (2026-09-25)

`SAVE` enters the same generated LED Owner as SET/GET. The UART translator
waits for the portable terminal outcome after an accepted receipt. It reports
`OK durable` only for `Outcome::Durable` after exact service readback; timeout
reports a pending key and preserves uncertainty. The target currently has no
qualified boot slot reader, controller pump or durable external wear authority,
so it does not attach a persistence service and returns `ERR save-unavailable`.
`GET` asks the same Owner for active values and a separate selected durable
record view. An unattached target says `saved=unavailable`; a service with no
selected record says `saved=no-record`, without guessing whether media is empty.
The retained flash handle is constructed once and moved to the UART service;
logical reset never acquires another. The linker limits all load bytes to
bank0 and names the bank1 settings regions. This is source preparation, not a
write-enabled image or device result.
The UART service captures the chip's read-only flash observation once after
startup. `STATUS` reports only its necessary `flash_basic` result; the bounded
read-only `FLASH` request reports the raw factory/FRI/status/protection words.
Keeping these responses separate preserves the 640-byte UART bound even when
STATUS contains all exact schedule identities.
Its `flash_basic` field is a necessary check only; it does not permit media
reads, erase or program and requires independent read-only device review.

To activate: independently review read/ECC and controller idle/protection
evidence, 32 MHz FRI RWAIT>=1, flash execution closure, bounded deadlines and
external durable wear receipt; construct one service from both classified
2048-byte slots and pass its selected record to `LedController::new_with_config`.
Use the fixed UART SAVE receipt/ResolveSave action and pump each backend command
outside the HSM. Preserve the default/corrupt/unsupported classification.
