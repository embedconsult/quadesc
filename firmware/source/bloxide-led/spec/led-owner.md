# LED owner output fault refinement

## Restored boot intent

For every valid restored `LedConfig`, the one Owner's initial readback and the primed output schedule derive `commanded_on` from `duty_permille != 0`. Duty zero boots commanded off and duty 1000 boots commanded on. Construction leaves revision zero and one initial phase epoch; it does not publish a second schedule or apply a synthetic write. Generated Start/Read must report the same intent as the primed schedule.

The generated owner keeps one canonical calibration state and one retained software outcome. A changed Apply reserves the fixed output slot before committing, then publishes the complete schedule. A same-value Apply does not reserve, increment the revision or restart phase.

The output service reports successful installation through the two-entry completion FIFO. On a readback failure during installation, it reports `Faulted(schedule)` in the reserved completion entry and latches `OutputFault { schedule, Installation, Pending }`. On a later edge failure, the earlier installation disposition remains valid and the latch records `OutputFault { schedule, Edge, Pending }`. Both paths close changed-output admission before the service attempts inactive output. The service records `Verified` only when the inactive readback succeeds; otherwise it records `Unknown`. The fault latch stays readable until explicit acknowledged resume; queued work stays fixed until a completion entry is available for truthful retirement. No physical result rewrites the earlier software Applied outcome.

On a latched fault, the consumer may retire a queued Ready schedule through the existing quiesce operation while admission stays Faulted. It places `CancelledByLifecycle` with B's complete key in the two-entry FIFO only when capacity exists; otherwise Ready remains intact until the producer drains a completion. A previously installed A stays `Installed`, with its later edge fault separately latched. The consumer repeats bounded retirement polls after a fault. No queued command is installed during this period. The producer may explicitly request quiesce from Faulted, drain both dispositions, and acknowledge the fault only after the queued and in-flight work is gone and inactive output has verified readback. The service acknowledges Quiesced only after that fault acknowledgement, an independently confirmed inactive output, and an empty completion FIFO. A requested new output epoch remains closed until the service explicitly accepts resume. Accepting resume clears the acknowledged old fault; it does not retry old schedules or reset/reacquire a device. Unknown safe-off cannot be acknowledged or resumed.

The status reader is read-only and shares the same one-shot split slot. Fixed status exposes the queued schedule, both pending terminal dispositions, and the last drained disposition so exact operation keys remain reconcilable across the FIFO boundary. It has no raw actor access, calibration copy, HAL type, executor handle or heap storage. It reports readback state, never optical observation. Software `Applied`, revision and phase remain owned by the calibration Owner and never change because of physical retirement.
# UART SAVE status action (2026-09-25)

The existing Owner accepts `Save` and the selected portable service owns
backend commands. `ResolveSave(key)` reports a separate terminal durable
record sequence and historical saved revision. The profile slot retains the
exact UART outcome before the existing profile observer releases its service
and broker custody. Pending, failed, indeterminate, rejected, stale and
unavailable are distinct. A request receipt cannot stand in for durable
completion. `ReadSaved` returns the selected portable record's sequence,
source epoch/revision and values; `NoRecord` does not claim empty media.
No second settings owner or persistence library is introduced.
