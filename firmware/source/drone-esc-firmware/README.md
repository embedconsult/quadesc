# Drone board application source

This is the authored schema 7 application composition for the SoM and QuadESC.
The customer release contains 252 parameters and the CAN XCP demo selected by
[`examples/am13-xcp-control/system.toml`](examples/am13-xcp-control/system.toml).
The generated executable is `can-xcp-control-am13`, targeting
`thumbv8m.main-none-eabi` with the application origin at `0x8000`.

[`mcu-xcp-platform`](mcu-xcp-platform) owns hardware initialization, fixed transport
slots, driver/PWM control, calibration storage and lifecycle shutdown. The LED
owner, XCP Session actor and framework are supplied as local source dependencies.
[`characterization`](characterization) contains the portable driver, PWM, ADC and
thermal contracts; [`host-all-motor-regression`](host-all-motor-regression) tests
the concrete board adapter without operating hardware.

Startup leaves PWM and both enable controls inactive. Raw driver faults for all
four motors interlock bench operation. Failed GPIO-off confirmation exposes
`drv_enable_state_known=0` and fences actuation, SPI, SAVE and reboot; an explicit
disable command can retry in a live session. An uncertain lifecycle completion
fences the transport and requires external safe-off and deliberate recovery.
The engineering manual explains that distinction and the bounded bench workflow.

Build, test and artifact commands run from the handoff root:

```sh
bash tools/firmware/build.sh NEW_BUILD_DIRECTORY
bash tools/firmware/test.sh NEW_TEST_DIRECTORY
python3 -B tools/firmware/verify.py
```

The build uses the shipped locks without resolving newer dependency versions.
Authored Rust, TOML and generator sources are retained here. Generated Rust and
Cargo scaffolding are disposable build outputs; the delivered BIN, AB1, ELF,
A2L and JSON metadata are under [`firmware/application`](../../application).
Calibration storage and loader ABI1 retain their existing layouts. SAVE is
available for persisted LED/ADC calibration fields while shutdown is confirmed.

See the [firmware overview](../../README.md), [portable build instructions](../../../tools/firmware/README.md),
[CAN tools](../../../tools/can/README.md), [engineering manual](../../../documentation/html/engineering/index.html)
and [source provenance](../../source-provenance.json) for the complete customer workflow.
