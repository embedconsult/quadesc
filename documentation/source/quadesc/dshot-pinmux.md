# Experimental: DShot transmit/receive pinmux switching

On QuadESC, receive a flight controller's command, transmit telemetry in the permitted reply slot, then release the wire and return to reception. This experiment changes each DShot pad's peripheral function at runtime between edge capture and timed output.

AM13E provides the mux, PWM and capture resources for this experiment. A complete bidirectional DShot implementation still needs finite frame generation, edge buffering, decoding and measured turnaround timing. The steps below describe an experimental implementation for your application.

(decide-which-end-the-som-is-playing)=
## Command and reply direction

QuadESC normally waits with the line released and command capture armed. The flight controller sends the command; QuadESC drives the line only during its telemetry reply slot.

Bidirectional DShot returns telemetry on the command wire. Commands and replies have different encodings: the controller sends a 16-bit command frame; the reply uses GCR encoding and transition coding. Bidirectional command signalling is inverted, with an idle-high line. Match polarity, checksum and reply decoding to the selected peer. These protocol details are described in [Betaflight’s DShot API documentation](https://betaflight.com/docs/development/API/Dshot).

On QuadESC, the separate `FC_Telemetry` signal is another connection. It is not the same wire as the bidirectional reply described here.

## Pads and peripheral choices

QuadESC’s four FC command pads each offer output and capture routes. The table allocates distinct resources for commands and replies. “PF” is the IOMUX peripheral-function value from TI’s pin table. These selections give you a starting point for the experiment.

| Signal | SoM pad / package pin | SoM J1 / QuadESC J9 pad | QuadESC FC header | Proposed TX output / PF | Proposed RX capture / PF |
|---|---|---|---|---|---|
| DSHOT1 | PA27 / 50 | H3 | J6.5 | MCPWM4_2B / 2 | TIMG12_0_CCP1 / 4 |
| DSHOT2 | PA4 / 26 | A1 | J6.6 | MCPWM4_3B / 3 | TIMG4_0_CCP1 / 4 |
| DSHOT3 | PB28 / 59 | J4 | J6.7 | MCPWM4_2A / 5 | TIMG4_0_CCP0 / 2 |
| DSHOT4 | PA26 / 47 | J1 | J6.8 | MCPWM4_3A / 6 | TIMG12_0_CCP0 / 4 |

The connector names in the FC-header column belong to **QuadESC**; the SoM has its own connector numbering. See [QuadESC connections](connections.md) and the <a href="../som/connectors.html">SoM connector reference</a>.

This allocation gives the four wires distinct output signals and distinct CCP inputs across two TIMG instances. Some alternatives alias: PA27 and PA4 both offer TIMG4_0_CCP1; PB28 and PA26 both offer TIMG4_0_CCP0. Selecting those alternatives on both pads would not create independent capture channels. Likewise, PA26 also offers MCPWM4_2A, already used here by PB28; the proposed table chooses its 3A alternative.

All four proposed outputs share MCPWM4’s time base, and A/B outputs in each pair share pair-level features. Two receivers share each TIMG counter. Start with **one wire**. Before expanding to four, arrange channel-specific capture and output control or coordinate a quiet interval for the whole shared peripheral. Disabling a shared timer while another wire is active can disrupt that exchange. The paired-output limits are explained in the <a href="../som/timing.html">SoM timed-output guide</a>.

TI lists these pad functions in the [AM13E230x datasheet](https://www.ti.com/lit/ds/sprspc3/sprspc3.pdf), Table 5-2. The direct TIMG receive paths are available in hardware; their buffering and service rate at the chosen DShot speed remain part of the experiment.

## Release the wire before receiving

An output that is low is still driving. Stopping a peripheral can also leave its last level latched by IOMUX. Neither action alone establishes a released line.

TI’s runtime switching procedure in the [technical reference manual](https://www.ti.com/lit/ug/sprujf2/sprujf2.pdf), §17.2.1, is:

1. Disable the currently connected peripheral function.
2. Clear the pad’s `PC` and `INENA` connection bits.
3. Write `PF = 0` to reset the mux data-path logic.
4. Select the new peripheral function.
5. Set the required connection and input-enable bits.
6. Enable the newly selected peripheral.

The `PF = 0` state disables the pad’s output transistors. Pull resistors are controlled separately and may remain connected. Clearing a GPIO output-enable bit only controls the GPIO output path; it is not a substitute for disconnecting an active PWM peripheral.

For transmit-to-receive turnaround, first let the complete final symbol reach the wire, then stop further transmit events and follow that mux sequence. DMA transfer completion may only mean the last value was handed to a peripheral; establish that the waveform itself has finished, including its final bit interval. Keep the new receive channel configured as an input before reconnecting it.

## Prepare capture while the line is released

For the direct TIMG route, configure the clock and timer range, capture mode (`COC`), input direction (`CCPD`), edge condition (`CCOND`) and input selection/filter (`IFCTL`). TI gives the edge-time capture setup in §18.2.3.1.2.1. A timestamp stream capturing both edge polarities is a useful starting point for inspecting the waveform.

While capture acceptance and its interrupt/DMA requests are disabled, initialize the receive buffer and configure the input path. After the mux transition, clear stale capture/event flags and any owned pending interrupt, reset the software frame state, and arm the receive timeout before enabling capture. This prevents an old completion from looking like the first event of a new command. Keep this final arming section bounded so it finishes before the peer’s first edge.

The selected TIMG input passes through a synchronizer. TI requires the input state to last longer than one TIMCLK period and describes additional capture latency (§18.2.3.1.1.1). Check the shortest expected pulse against the actual timer clock and filter settings. A filter that removes noise can also remove real protocol edges.

Capture registers are not a complete packet buffer. Drain timestamps through a suitable DMA or interrupt path before they are overwritten, account for timer wrap, and detect incomplete frames. Measure interrupt or DMA service time against the shortest interval between edges. As an alternative experiment, GPIO input can feed eCAP through INPUTXBAR (TRM §20.1 and chapter 24); that route still needs a resource and buffering plan.

## One exchange, two ownership changes

```text
FC TX:       [ complete command ] | released....................[ next command ]
QuadESC RX:  [ capture + decode ] | disabled.....................[ capture ... ]
QuadESC TX:  released.............| reply slot [ telemetry ] | released
                                                          → rearm RX
```

The diagram gives ordering, not fixed delay values. Choose the reply window, idle interval and receive timeout from the flight controller's protocol implementation. The nominal command bitrate alone does not specify the available mux-switching time.

The following is **pseudocode**, not runnable register code. `switch_mux_as_TI_requires` means the disconnect/reset/select/reconnect sequence above.

```text
esc_exchange():
    begin input/released, capture armed with a bounded receive timeout
    capture command edges; reject incomplete frames or invalid checksum
    if no valid command: stay released, rearm reception and return

    if this exchange calls for a reply:
        disable capture requests; disconnect PC and INENA; select PF=0
        prepare finite encoded telemetry and its intended idle level
        switch_mux_as_TI_requires(TX, intended_idle_level)
        transmit within the flight controller's permitted reply slot
        wait until the final symbol completes on the wire
        disable further TX waveform/DMA events
        disconnect PC and INENA; select PF=0

    disable capture acceptance and its interrupt/DMA requests
    configure RX input, edge buffer and timeout while disabled
    select RX mux and reconnect input with capture disabled
    clear stale capture/event flags and owned pending interrupt
    reset frame state; arm timeout; enable capture
    leave the pad input/released while waiting for the next command
```

Do not transmit outside the controller's reply slot. A malformed command or receive timeout leaves the wire released. Prepare the receive path early enough to capture the first edge of the next command, and keep error paths bounded so they cannot leave output drive enabled indefinitely.

## Verify the signal exchange

Begin with a single signal and common ground, using compatible logic levels and a known peer or waveform source. For a QuadESC signal experiment, keep the motor gate enables inactive; the pinmux exchange can be investigated without energizing a motor.

Use an oscilloscope to see analog levels and a logic analyzer to compare decoded edges. If available, mark the software’s “TX complete,” “released” and “capture armed” points on a spare debug output. Those markers reveal scheduling latency; the signal trace establishes what actually happened on the wire.

| Check | What to look for |
|---|---|
| Finite transmission | Exactly one intended frame, with the final symbol intact and no trailing timer pulse |
| Line release | The output is high impedance during peer ownership; confirm with a suitable weak bias or current-limited fixture, not an idle level alone |
| Contention | No opposing drive or abnormal intermediate level during either handoff; use a current-sensitive fixture if needed to distinguish two drivers at the same level |
| Mux glitches | No extra threshold-crossing edge at disconnect, reconnect or return to TX |
| First-edge capture | The first command edge and every later edge match the external trace within the expected capture resolution |
| Buffer servicing | No capture overwrite or lost timestamp under the intended interrupt/DMA load |
| Decode | Known command patterns decode correctly; invalid checksum and incomplete commands are rejected; the FC decodes the transmitted telemetry |
| Error recovery | Missing command, malformed command and timeout leave the pad released and allow a later deliberate exchange |
| Turnaround | Worst observed release/arming time fits inside the verified peer timing with margin |

Repeat with different data patterns, especially shortest pulse widths, then vary the delay of the next command's first edge. Test simultaneous activity only after the single-wire transition is clean. Look for a repeatable handoff and a complete timestamp stream before moving on to the full exchange.
