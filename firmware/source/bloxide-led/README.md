# Bloxide LED host precursor

The separate host XCP demo composes this generated LED Owner with the generated XCP Session. Its additive `XcpProvider` action translates typed requests into the existing sole Owner, retained outcomes and output admission; it does not select Profile P or SAVE. The frozen Apply/Read/Release scalar interface remains intact.

This private feature package contains plain LED messages, validated scalar values, the portable calibration owner action context, a split-once output slot, absolute-time phase calculation, and a pure TOML `led-owner` blox. It reuses the pinned `bloxide-calibration` `Owner`, `CommitGate`, and `OperationKey` source supplied by this diagnostic hub. The local Cargo path dependency is explicit for this bounded composition; a releasable independent package still needs a reviewed source distribution choice.

Only `LedController::apply` changes the active configuration. A changed value reserves the output slot before the T07 owner commits and publishes an `OutputSchedule` before the retained Applied result is visible. A same-value operation does not reserve a slot or reset phase/revision. The split producer and consumer are moved to distinct owners; no actor Reset splits the slot again. `phase_at` computes the current logical phase from an absolute epoch, including late polls and constant duty extremes.

The output slot supports one queued schedule, one service-owned schedule and two retained dispositions. Quiescence closes admission and retires a queued schedule with a disposition; an explicit consumer acknowledgement opens a requested new output epoch. Host tests cover this path. Physical GPIO completion, electrical state, timing and fault recovery remain target integration obligations.

Source files here are authored for this precursor. No license is asserted for this new repository. The framework and calibration inputs retain their own source notices and are not copied into it.

For the hosted diagnostic, a second split-once bounded reply slot passes typed action outcomes to the serial service. An occupied reply remains intact and latches overflow instead of overwriting a result; the serial session stops admission if that invariant fails.

This integration branch adds a selected restricted Profile P action and a UART SAVE action to the same declarative `led-owner` blox. `LedController` still contains the sole calibration `Owner<LedConfig>`. Profile P borrows that Owner only inside the generated action; scalar writes use the existing validated apply path and output reservation. A fixed `ProfileSlot` holds the reviewed protocol/service/Broker state, while the application pumps backend commands outside the action. A host-only simulator supplies media for source tests. Without an attached slot, SAVE is unavailable and Profile P is fenced, including in the selected AM13 target composition. No fault recovery control was added.
