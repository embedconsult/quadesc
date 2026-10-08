# Motor-control design and shared resources

QuadESC’s pin allocation was chosen to support field-oriented control (FOC): each motor has a PWM time base, and each three-phase feedback group is spread across ADC0, ADC1 and ADC2 so its phases can be measured together. The feedback connections are dedicated to analog sensing, with digital control carried on separate signals.

The drive path is **SoM PWM → DRV8323 → half bridges → motor phases**. SPI configures and inspects the four drivers, nFAULT reports digital fault states, and the ADCs receive phase currents, phase voltages and slower monitoring signals. These connections provide the hardware foundation; a closed-loop application supplies the modulation, acquisition schedule, rotor angle and control algorithm.

## Why one PWM module per motor?

Each motor’s three phase commands share a carrier period. Giving each motor its own MCPWM module keeps that relationship inside one timer while allowing the motors’ periods and phase offsets to be controlled separately. The modules can also synchronize, which is useful when arranging their sampling windows.

| Motor | PWM module | Phase A: pad / output | Phase B: pad / output | Phase C: pad / output |
|---|---|---|---|---|
| 1 | MCPWM1 | PB6 / 1A | PB7 / 2A | PB8 / 3A |
| 2 | MCPWM3 | PC25 / 1A | PC26 / 2A | PC18 / 3A |
| 3 | MCPWM0 | PB13 / 1B | PA10 / 3A | PA25 / 3B |
| 4 | MCPWM2 | PC1 / 1A | PC3 / 2A | PA24 / 3A |

The mux selections for phases A/B/C are Motor 1 **5/5/5**, Motor 2 **4/4/4**, Motor 3 **5/7/13**, and Motor 4 **7/7/3**. Follow the [connection reference](connections.md) for connector access. These routes also leave the SoM’s boot UART, boot invoke, SWD, CAN and I2C connections clear of the motor PWM and feedback signals.

Separate timers do not isolate the motors electrically. Gate enables, ADC cores and the power supply remain shared. Any carrier staggering must still fit the available ADC time and the current-sensing windows.

## Motor 3’s 1B / 3A / 3B outputs

The peripheral letters describe timer outputs, not motor phases or gate-driver high/low inputs. Motor 3’s PB13 output **1B** drives phase A’s **INHA**. PA10/3A drives INHB, and PA25/3B drives INHC.

Pair 3 has separate CMPA/CMPB compares and AQCTLA/AQCTLB action controls. In single-edge or symmetric up/down PWM, 3A and 3B can therefore carry separately controlled duties. Pair 1 provides the third command on 1B. Preserve those distinct signals through the output path: using pair 3 as a complementary dead-band pair derived from one signal would couple the two commands.

This arrangement shares MCPWM0’s period and pair-level features. It supports the documented separate-duty modes; arbitrary asymmetric edge placement has different resource requirements. See the SoM’s <a href="../som/timing.html">paired-output explanation</a> for the distinction.

PB12 carries the FC current-telemetry PWM through MCPWM1_3B. It shares Motor 1’s period and the PWM3 pair with phase C on PB8. Its duty can use the pair’s separate compare resource in the appropriate mode, but changing its carrier period also changes Motor 1’s timing.

## From a PWM window to a current vector

FOC uses the phase-current vector together with the rotor’s electrical angle. A Clarke transform expresses the phase currents in stationary α/β coordinates; a Park transform rotates those components into the rotor-oriented d/q frame. The controller can then regulate the components associated with flux and torque and produce the next phase commands. TI’s [FOC overview](https://www.ti.com/zh-tw/video/3881563245001) introduces this sequence.

The currents should describe the motor at one instant. If phase A is sampled before a switching edge and phase B after it, their combination contains time skew as well as the actual current vector. QuadESC spreads A/B/C across three ADCs to support simultaneous three-phase sampling for this reason.

A useful control cycle is:

```text
PWM establishes a valid sensing window
    → one event starts A/B/C current acquisition across ADC0/1/2
    → completed samples are offset/gain corrected
    → Clarke + Park use the corresponding rotor angle
    → current control computes the next duty commands
    → shadowed PWM values take effect at the chosen boundary
```

Choose a stable part of the cycle where the low-side shunts represent the required currents, after switching transients and amplifier settling. Counter zero or period is useful only if the selected modulation makes that point suitable. At extreme duties, the valid window can shrink or disappear; the control strategy must handle that condition. The [sensing page](sensing.md) explains the separate current and voltage triplets and the four-motor schedule.

## Shared enables, separate selects

`DRV_ENABLE` and `INL_ENABLE` are common to all four channels. A software motor selector does not isolate them per motor. Four chip selects separate SPI register transactions, while nFAULT inputs identify driver fault indications.

| Motor | Driver | Chip-select MCU pad | nFAULT MCU pad |
|---|---|---|---|
| 1 | U1 | PC13 | PA30 |
| 2 | U2 | PB14 | PB15 |
| 3 | U3 | PB16 | PB17 |
| 4 | U4 | PC5 | PB0 |

Initialize `DRV_ENABLE` on **PB10** and `INL_ENABLE` on **PB11** low, with chip selects high. Configure UC3 MOSI/MISO/SCLK on **PC6/PC4/PC2**, mux **6**, for **16-bit SPI mode 1** driver transactions. Allow for driver wake time, valid supplies and self-clearing command bits when implementing register transactions. Selected-driver readback is meaningful only when that driver has the required supply and wake state.

Stopping PWM, disabling the common gates and requesting zero torque have different effects. Give each an intentional output level and order, and decide how a driver fault affects the other motors. A closed-loop application also needs consistent current signs, rotor sensing or estimation, bounded computation and a fault response. See [firmware compatibility](characterization.md) for application requirements.

The mux and paired-output capabilities are described in TI’s [datasheet](https://www.ti.com/lit/ds/sprspc3/sprspc3.pdf), Table 5-2, and [technical reference manual](https://www.ti.com/lit/ug/sprujf2/sprujf2.pdf), §26.1 and §26.5–26.7.
