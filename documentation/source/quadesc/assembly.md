(mechanical-assembly)=
# Mounting and cooling

The QuadESC PCB is **60 × 48 mm**. Allow room on both sides for components, the installed SoM, connectors and wiring. The PCB outline alone is not the full assembly envelope.

## Fit the board into your system

Use the [board views](overview.md#board-views) to plan access to battery terminals J1, motor terminals J2–J5, FC connector J6 and debug header J7. Leave access to the installed SoM's reset and BSL connections for programming and recovery.

Keep conductive mounts and enclosure surfaces clear of exposed pads and components. Support the battery and motor wires so movement does not pull on their solder joints. Leave clearance for the underside components and module rather than resting the board directly on a flat conductive surface.

## Plan for cooling

Leave airflow around the four power stages and allow heat to escape from the enclosure. Board temperature depends on current, airflow, ambient temperature and load duration; use the [temperature scale](sensing.md#board-temperature-from-the-thermistors) to monitor each stage while developing your application.

The [electrical targets](characterization.md#electrical-targets) describe the intended operating range. Establish the usable current for your mounting and cooling arrangement before sustained motor operation.

## Access for service

Disconnect battery power before servicing wiring or the board. Keep programming connections accessible after installation so an application can be replaced through BSL or SWD. See [care and service](maintenance.md) for workbench checks after a repair.
