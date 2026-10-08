# Timed outputs, capture and clocking

The SoM can generate precisely timed outputs, measure incoming edges, and use timer events to start analog conversions. These functions are useful together: a timer can establish the rhythm of a control loop while the ADC measures the circuit at a chosen point in that rhythm.

Start with the [connector reference](connectors.md) to find the pad, then choose its peripheral function. A pad’s mux selection, the timer channel, and the timer’s shared time base all matter.

## Choose the resource that fits the job

| Resource | What it provides | What to plan around |
|---|---|---|
| MCPWM0–4 | A dedicated time base per module, up to three pairs of PWM outputs, compare events and ADC triggers | Outputs within a module share its counter and period |
| TIMG capture/compare | Timed outputs and timestamp or pulse-width capture on supported CCP pins | Two pads can expose the same CCP input; they are alternative access to one resource |
| eCAP | Edge capture, with input selection through the input crossbar | Capture storage and servicing must keep up with the incoming edges |
| IOMUX | Selects the digital peripheral connected to a pad | Runtime switching requires an explicit disconnect/reconnect sequence |

Select the exposed pad routes and peripheral functions for your application. Continuous PWM, finite waveforms and capture need different counter, event and servicing arrangements. Hardware capability alone does not provide a frame generator or decoder. The MCU’s TMU and NPU are also resources for application code.

## Period, duty and shared counters

A PWM period is set in timer ticks. The counter mode, selected clock and prescaler determine how those ticks turn into time. Up/down counting traverses both sides of the carrier, so use the selected mode’s timing relationship when calculating the period. Duty and pulse width are also quantized into ticks: inspect the effective timing returned by the driver and reject combinations the timer cannot represent.

One MCPWM module gives its outputs a common carrier. Their duty values can differ, but changing the period changes the timing of every output on that module. Separate MCPWM modules have separate counters and periods; they can run independently or synchronize with a programmed phase relationship. This is useful for coordinating several loads or staggering their switching and measurement windows.

Use shadowed compare updates and a deliberate load event so a new duty value takes effect at a consistent boundary. Decide how zero duty, full duty and stopping should appear at the actual connector. A requested zero value, a stopped counter and a disabled external load are different operations.

## A/B output pairs

Each MCPWM pair has two compare registers, CMPA and CMPB, and separate action-qualifier controls for its A and B outputs. The action qualifiers determine what happens at zero, period and compare events on each counting direction.

| Pair configuration | Available waveform control |
|---|---|
| Single-edge PWM | Two outputs with separately controlled duties |
| Symmetric up/down PWM | Two outputs with separately controlled symmetric duties |
| Asymmetric dual-edge waveform | The pair’s compare resources serve one independently positioned waveform |
| Complementary output with dead band | The outputs have a deliberate relationship through the dead-band path |

To use A and B for unrelated duties, preserve their separate action-qualifier signals through the output path; a complementary configuration derived from one signal couples them. Pair features and the module’s time base remain shared. QuadESC’s <a href="../quadesc/control.html">motor-control page</a> shows a practical use of this distinction.

## Let the timer choose the sampling instant

MCPWM events can request ADC conversions without a software call at each sample. Select an event that falls within the circuit’s useful measurement window, then configure the ADC sequence and acquisition duration around it. A compare event is often more useful than “whenever the main loop gets here.”

A shared trigger can coordinate separate ADC cores. It does not clear work already queued on those cores, so include conversion time and arbitration in the schedule. Keep the sampling instant distinct from the later conversion-complete interrupt and the later output update. See [analog acquisition](analog.md) for settling and simultaneous sampling.

## Switching a pin between transmit and receive

A stopped peripheral can leave its last output level latched at the pad. Forcing an output low also continues to drive the wire. Use the documented IOMUX transition when changing who owns a shared signal.

For runtime switching, TI's IOMUX sequence is: disable the active peripheral function, clear `PC` and `INENA`, select `PF = 0`, select the new peripheral function, restore the required connection/input-enable bits, then enable the new peripheral. `PF = 0` disables the output transistors; pull resistors remain separately controlled. Clearing GPIO output enable does not disconnect a PWM output.

Complete the final output interval before disconnecting. Configure the receive path while disabled, clear stale events, and arm capture before the first expected input edge. Account for shared timer resources when disabling a peripheral. See TRM §17.2.1 for the mux sequence.

## Clock domains and first measurements

SYSOSC provides a 32 MHz clock source, and the external crystal is nominally 25 MHz. Select and initialize the clock source required by your application. A timer’s functional clock must be checked separately from the CPU frequency. CAN can also use a separate functional clock, so calculate its bitrate from that selected domain.

When trying a timed output, measure the carrier period, both extreme duties, the first pulse after start, and the level after stop. Repeat after a clock change. A peripheral reset can clear its power state, so follow the selected driver’s power and clock initialization order.

TI describes these resources in the [AM13E230x datasheet](https://www.ti.com/lit/ds/sprspc3/sprspc3.pdf), Table 5-2, and the [technical reference manual](https://www.ti.com/lit/ug/sprujf2/sprujf2.pdf), chapters 17, 18, 20, 21, 24 and 26. For paired PWM outputs, see §26.1, §26.5–26.7.
