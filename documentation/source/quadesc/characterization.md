# Specifications and compatibility

QuadESC combines a four-channel power stage with a reusable controller module. The values below describe the power-stage design targets and help explain the intended battery range and cooling needs.

## Electrical targets

| Parameter | Design target |
|---|---|
| Battery | 3S–6S, 9–25.2 V |
| Design maximum | 32 V |
| Continuous current per motor | 40 A |
| Burst current per motor | 70 A |

**The current targets are not established operating ratings.** Allowable current depends on cooling, ambient temperature and load duration. No burst duration is specified for the 70 A target, and the 32 V design maximum is not a recommended supply setting. Plan operation around the battery range and the thermal behavior of the complete assembly.

## Firmware compatibility

Factory ROM provides TI BSL. Use the <a href="../som/firmware.html">SoM programming guide</a> to load your application. Motor operation needs code that sequences the gate drivers, acquires feedback at the right time and handles faults.

For flight-controller use, the command decoder and telemetry format need to match the connected controller. The DSHOT labels identify signal routes; the application must configure capture and decoding to receive commands. See [FC integration](integration.md) and [shared control resources](control.md) for the hardware relationships.

## Measurement limits

Current and voltage conversions depend on reference voltage, component values and the gate driver's current-sense gain. The formulas in [sensing](sensing.md) use nominal values. Check the actual reference and gain when comparing a reading with an instrument.

The [temperature scale](sensing.md#board-temperature-from-the-thermistors) gives a nominal ADC-to-Celsius estimate for all four channels. Confirm the fitted thermistor's curve before using that estimate for thermal control. The sensors measure nearby board temperature. Low-side current samples also depend on switching state, so their timing matters as much as their scale.

## Understanding thermal behavior

Board copper, airflow and the duration of a load all affect temperature. A short pulse and a sustained load at the same current produce different heating; repeated pulses can accumulate heat. Compare channels under similar supply and cooling conditions, and watch temperatures as load increases.

Startup, reset and fault handling affect the gate waveforms as well as steady operation. Check those transitions when evaluating a motor-control application. The [power chapter](power.md) explains why shunt heating rises quickly with current.
