# Care and service

Disconnect the supply and loads before working on the board. Begin with the connections and parts near the symptom, then check the affected circuit before returning to motor operation.

## Service checks

Inspect repaired joints and check continuity while unpowered. Next, power the logic and check the supply rails and analog reference with motor loads disconnected. Look at driver responses, fault inputs and enable levels before trying a controlled load.

A changed shunt, divider or current-sense gain changes the conversion from ADC counts to engineering units. Compare the affected channel's raw reading and reference with the formulas in [sensing](sensing.md). A change in the module or timer configuration can also affect output timing.

(firmware-and-replacement-modules)=
## Firmware and controller service

The SoM is installed as part of QuadESC. The [mounting guide](assembly.md) covers clearance, cooling and programming access. The <a href="../som/maintenance.html">SoM service guide</a> covers module-specific rework checks.

Use an application suited to the mainboard's shared enables, driver connections and feedback routes. The [compatibility guide](characterization.md) describes the requirements for motor-control code, and the <a href="../som/boot.html">SoM boot guide</a> covers programming access.

## Getting help

Describe what you connected and what you observed. A supply voltage, load description and measurement point are useful for an electrical problem; a short sequence that reproduces the symptom helps with software behavior. Say whether a component reference belongs to the mainboard or the SoM: each board has its own U1, U2 and other designators.

## Glossary

| Term | Meaning |
|---|---|
| CSA | Current-sense amplifier inside the gate driver |
| Half bridge | High-side and low-side switching pair for one phase node |
| BEMF | Back electromotive force; also used in phase-voltage net names |
| Kelvin sense | Separate low-current sense connection to a current shunt |
| nFAULT | Active-low driver fault signal |
| FC | External flight controller |
| FOC | Field-oriented control |
| Nominal scale | Conversion formula using nominal component and reference values |
