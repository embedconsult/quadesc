# Sensing and engineering units

QuadESC supplies **29 logical analog signals**: twelve currents, twelve phase voltages, four thermistor nodes and battery voltage. The phase-feedback connections are dedicated to analog sensing. Each motor’s A/B/C currents are assigned to ADC0/1/2, and its A/B/C phase voltages follow the same split.

This arrangement was chosen for FOC: the three phases of a measurement group can be sampled at the same moment. Current and voltage are separate groups, and the four motors share the three converters. The route reference lists 30 ADC selections; PA28 / J9 K1 is spare on this carrier.

## Phase routes at a glance

Each entry shows **MCU pad / channel number** within the ADC named in the column.

| Motor and measurement | Phase A — ADC0 | Phase B — ADC1 | Phase C — ADC2 |
|---|---|---|---|
| Motor 1 current | PC14 / 18 | PC12 / 25 | PA29 / 5 |
| Motor 1 voltage | PC15 / 19 | PC11 / 24 | PC9 / 2 |
| Motor 2 current | PA16 / 15 | PA18 / 12 | PA7 / 22 |
| Motor 2 voltage | PA17 / 12 | PB4 / 5 | PB5 / 24 |
| Motor 3 current | PB24 / 21 | PB30 / 30 | PB26 / 7 |
| Motor 3 voltage | PB27 / 27 | PB1 / 7 | PB29 / 10 |
| Motor 4 current | PA3 / 4 | PA8 / 18 | PA2 / 25 |
| Motor 4 voltage | PB3 / 9 | PA9 / 19 | PC7 / 4 |

| Slower signal | MCU pad | ADC route |
|---|---|---|
| Motor 1 thermistor | PC10 | ADC1 / 16 |
| Motor 2 thermistor | PA5 | ADC1 / 13 |
| Motor 3 thermistor | PB25 | ADC1 / 22 |
| Motor 4 thermistor | PC8 | ADC1 / 23 |
| Battery voltage | PB31 | ADC2 / 29 |

These are converter channel numbers, not physical AIN numbers. For example PC14 is physical AIN26, connected to ADC0 channel 18. The SoM’s <a href="../som/analog.html">analog guide</a> explains both namespaces.

## Sample a triplet together

An MCPWM event can trigger all three ADCs for one motor’s current triplet. Give them matching acquisition windows and make sure none is occupied with background work when the event arrives. Each ADC has one sample-and-hold, so the three phases can be acquired together without taking three consecutive samples through one converter.

For FOC, that timing keeps the phase currents aligned before Clarke/Park transforms form the current vector. Choose the window where the relevant low-side shunts actually carry the phase currents. Leave time after switching edges for ringing and current-sense settling, and keep the entire acquisition interval inside the valid window. TI discusses the switching-state dependence of low-side measurements in its [current-sensing application brief](https://www.ti.com/document-viewer/lit/html/SBOA160).

```text
One motor's cycle — conceptual, not to scale

Switching edge     settled, shunt-valid interval          next edge
      │           ├────────────────────────────┤              │
ADC0: IA                    [ acquire ] → convert
ADC1: IB                    [ acquire ] → convert
ADC2: IC                    [ acquire ] → convert
                             same window

Phase-voltage A/B/C: another coordinated acquisition window
```

A useful window depends on modulation and duty, not just the timer’s counter value. Near the duty limits, a control strategy may need to adjust modulation or reconstruct a current when its measurement is unavailable. Sampling all three inputs at once does not make an invalid shunt reading valid.

## Fit four motors and monitoring into the schedule

The twelve current inputs need at least four triplet acquisitions to visit them all. The twelve phase voltages need another four. They cannot all be captured at one instant with three sample-and-hold circuits.

One possible schedule uses a motor’s PWM event for its current triplet, another window for its voltage triplet, and then services the other motors’ windows. Coordinating or staggering the PWM carriers may help spread those requests. Choose the ordering, phase offsets and loop rate to fit conversion time and the available sensing windows.

Put temperature and battery acquisitions in background slots. Temperature adds work to ADC1, which also measures every phase B signal; battery adds work to ADC2. Sequencer priority alone cannot remove a conversion already in progress. Check that all three cores are ready at the critical trigger and detect missed or overflowing requests.

A diagnostic sequential scan is useful for checking levels and wiring. It is not the simultaneous acquisition schedule needed by a FOC loop. See [motor control](control.md) for the relationship between sampling, rotor angle and PWM updates.

## Current direction and gain

The inverting current-sense amplifier uses:

```text
Vadc = raw × Vref / 4096
Ishunt = (Vref / 2 − Vadc) / (CSA_gain × Rshunt)
```

With a **3.3 V reference, 20 V/V gain and 1 mΩ shunt**, the formula reduces to `Ishunt ≈ (2048 − raw) × 0.040283 A`. One count corresponds to approximately **−0.040283 A** and the nominal zero-code intercept is **82.5 A**. These are nominal conversion coefficients, not a measured calibration or channel rating. Positive current is defined from the low-side source through the shunt toward ground.

The assumed CSA configuration includes VREF/2 bias and normal shunt-sense mode. Check the actual driver register state before using the scale. Measure zero-current offsets for the channels and confirm current direction with a known condition. A low-side shunt does not continuously measure winding current throughout every PWM interval.

## Phase-voltage return matters

The 33 kΩ / 3.3 kΩ divider bottoms return to the low-side source rather than ideal ground. Therefore:

```text
Vphase_to_ground = 11 × Vadc − 10 × Vlow_side_source
```

Simply reporting `11 × Vadc` is an estimate. At 41.25 A through 1 mΩ, the omitted correction is **−0.4125 V**.

The `BEMF` inputs measure divided phase-node voltages. Their interpretation depends on the switching state and the acquisition window; the label alone does not make every reading an isolated back-EMF value. Allow for divider/filter settling when choosing the voltage acquisition time.

(thermistors-and-battery)=
## Board temperature from the thermistors

All four temperature channels use the same circuit arrangement. **Lower ADC counts mean a warmer board.** Each thermistor senses the board near its motor's power stage.

### Convert the raw count to Celsius

With a nominal **10 kΩ pull-up** and VAA used as the ADC reference, the thermistor resistance is:

```text
Rntc = 10000 × raw / (4096 − raw)    # ohms
```

For this conversion, use counts **1–4094**. Treat rail codes 0 and 4095 and any out-of-range value as invalid; 4096 is outside the 12-bit range and would divide by zero. Confirm that VAA and the ADC reference match before using this ratiometric formula.

The following nominal example assumes a **10 kΩ thermistor at 25 °C**, a **10 kΩ pull-up** and **B = 3380 K**, with VAA used as the ADC reference. Confirm the fitted thermistor's part and resistance/temperature curve before applying this scale to thermal control.

```text
T_C = 1 / (1 / 298.15 + ln(raw / (4096 − raw)) / 3380) − 273.15
```

`ln` is the natural logarithm. **2048 counts ≈ 25 °C**; **1000 counts ≈ 58 °C**.

| ADC count | Approximate board temperature |
|---|---|
| 3024 | 0 °C |
| 2048 | 25 °C |
| 1203 | 50 °C |
| 672 | 75 °C |
| 381 | 100 °C |

Use counts **1–4094**; **0 and 4095 indicate an invalid reading**. This nominal conversion does not establish the fitted sensor's calibration; accuracy also varies with reference matching, temperature and component tolerances.

{download}`Download ADC-to-Celsius helper <../../reference/thermistor-temperature.py>`

## Main battery voltage

Read **PB31, ADC2 channel 29**, reached through **J9 J5** on `VBAT_SNS`. The battery divider is **100 kΩ from +BATT to the input and 4.7 kΩ from the input to ground**:

```text
Vbattery = raw × Vref / 4096 × (104.7 / 4.7)
         ≈ raw × 0.0179475 V       # nominal Vref = 3.3 V
```

For example, **1000 counts ≈ 17.95 V**; a **25.2 V** battery gives approximately **1404 counts**. Use the actual reference voltage for better accuracy. The input has a 0.1 µF filter capacitor, so allow settling after a supply change and acquire it in a background slot rather than delaying a phase-current triplet. A zero reading with battery power present calls for checking the divider node, reference and ADC selection.

## Full route reference

The [analog route CSV](evidence.md) maps all 30 ADC selections through MCU pad, package pin and LGA pad to the mainboard net.

{download}`Download analog route CSV <../../reference/mainboard-analog-routes.csv>`

TI’s [datasheet](https://www.ti.com/lit/ds/sprspc3/sprspc3.pdf), Table 5-7, gives the analog mux paths. The [technical reference manual](https://www.ti.com/lit/ug/sprujf2/sprujf2.pdf), §21.1 and §21.3.1, describes the three-core sampling model and sequence arbitration.
