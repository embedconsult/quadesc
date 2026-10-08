# Analog acquisition deep dive

The SoM brings analog-capable pads to its carrier interface and has three ADC cores, ADC0, ADC1 and ADC2. Each core selects an input, samples it, and converts that sample to a 12-bit result. A carrier can use those three cores to capture three related signals at the same moment, or schedule them independently for slower measurements.

The route reference lists 30 ADC selections across the three cores. Configure the conversions and bounded completion waits in your application. The carrier circuit determines each input’s scale and units. Start with the [connector reference](connectors.md) to locate a signal, then follow its analog route.

## From a pad to a sample

| Name | Example | Meaning |
|---|---|---|
| MCU pad and package pin | PC14, package pin 45 | The physical MCU connection |
| Physical analog input | AIN26 | TI’s analog identity for that pad |
| ADC instance and channel | ADC0, channel 18 (`A0_18`) | The input selection used by that converter |
| SOC or sequence position | A configured conversion slot | When and how the selected channel is acquired |

For PC14, the full path is **package pin 45 → AIN26 → ADC0 channel 18**. These numbers need not match. Another example is PA3/AIN9, which can feed ADC0 channel 4 or ADC2 channel 21. Choosing a converter changes the acquisition resources used; it does not move the carrier wire.

Peripheral channel selection uses the ADC instance and channel, while the pad table identifies the physical AIN. Keep these namespaces distinct in your code. TI’s datasheet Table 5-7 gives the available analog paths for each package.

## Give analog signals their own connections

Route a sensing node to a dedicated analog connection and keep digital output activity off that signal. Configure its pad for analog operation, with digital drive, input-buffer activity and pull resistors disabled as appropriate. TI’s analog-pad initialization disconnects the digital peripheral path.

This keeps a sensor’s voltage from competing with a driven logic level or an unintended pull resistor. Also check the complete carrier connection: a pad exposed on more than one connector is still one electrical node. For example, PA28 reaches both the LGA interface and mikroBUS AN.

Choose protection, divider range, source impedance and filtering for the MCU’s analog requirements. A high-value divider or an RC filter needs enough acquisition time to charge the ADC’s sampling capacitor. Account for leakage and rail sequencing, keep the voltage within the analog input limits, and give reference and analog return currents a quiet path away from switching-load returns.

## Coordinated sampling across three ADCs

Each ADC has one sample-and-hold circuit. Assigning three related signals to different ADCs allows simultaneous acquisition when all three are ready and configured for a matching trigger and acquisition interval. Placing all three on one ADC makes their acquisitions sequential.

```text
Shared trigger       ┃
ADC0                 ┣━━ acquire signal A ━━┫ convert A → result
ADC1                 ┣━━ acquire signal B ━━┫ convert B → result
ADC2                 ┗━━ acquire signal C ━━┫ convert C → result
                     matching acquisition windows
```

Use matching acquisition timing and leave the cores free to accept the event together. Different pending work or sequence priorities can delay one core even when the trigger source is shared.

A second triplet needs another acquisition window. Three converters do not sample every connected input at once. This matters for any application comparing changing signals: a three-axis sensor, a multi-channel power measurement, or QuadESC’s <a href="../quadesc/sensing.html">phase feedback</a>.

## Triggers, sequences and background measurements

MCPWM provides hardware start-of-conversion events to the ADCs. Each ADC supports conversion slots grouped into up to four sequencers. A sequence visits its configured slots in order; simultaneous requests are handled according to priority and pre-emption settings.

A practical schedule separates time-critical samples from background work:

1. Choose the physical measurement window and its acquisition duration.
2. Assign each related signal to an available ADC and select the same trigger where simultaneous sampling is needed.
3. Budget conversion time, result handling and the next trigger before adding background inputs.
4. Place slower measurements where they will not occupy a core at the critical trigger.

Keep filtering and source settling in that budget. Raising the trigger rate cannot make a slow analog node settle faster. Check overflow and completion status so an old or missed sample is not mistaken for the latest one.

## Raw counts and useful units

For nominal 12-bit scaling:

```text
Vinput = raw × Vref / 4096
```

Use the measured reference when converting to volts. Shunts, dividers and thermistors add their own gain, offset or nonlinear conversion. Keep raw counts available while checking the circuit; they help distinguish scaling mistakes from acquisition problems.

Preserve device trim during initialization and bound completion waits so a missing conversion cannot hang your application. A code at 0 or 4095 can mean saturation, absent power or a legitimate boundary condition. Interpret it with the circuit and reference voltage. A resistance value alone does not determine temperature without the fitted sensor’s curve.

## What a diagnostic scan tells you

A sequential scan visits one route after another. Return a result or error for each acquisition so failures are visible to the caller. It is useful for checking connected inputs, approximate levels and slow changes. Its samples come from different instants. A synchronized control application needs its own acquisition schedule rather than treating that sweep as a simultaneous snapshot.

See [timed outputs and clocking](timing.md) for hardware triggers. TI’s [datasheet](https://www.ti.com/lit/ds/sprspc3/sprspc3.pdf), Table 5-7 and §6.9.1.1.3, describes analog routing and the input model; the [technical reference manual](https://www.ti.com/lit/ug/sprujf2/sprujf2.pdf), §21.1 and §21.3.1, describes the sample-and-hold cores and sequencing.
