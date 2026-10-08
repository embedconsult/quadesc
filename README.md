# QuadESC

QuadESC is a four-motor power board with an installed AM13E system-on-module. This repository contains the editable mainboard and module designs, schematics, firmware sources and hardware manuals.

## Hardware documentation

The handoff manuals are maintained as Markdown in [`documentation/source/`](documentation/source/):

- [QuadESC manual](documentation/source/quadesc/index.md): power, connections, sensing, control resources and mechanical integration.
- [AM13E SoM manual](documentation/source/som/index.md): module interfaces, pin assignments, power selection, TI BSL programming and debug.

Connector references retain pin numbers, net names and electrical mappings. Use the KiCad designs for physical board geometry.

See [documentation build instructions](documentation/README.md) to generate the HTML site locally. CI builds and checks the same source for merge requests and pull requests, and publishes the manuals from `main` through GitHub Pages on [embedconsult/quadesc](https://github.com/embedconsult/quadesc), with an optional GitLab Pages pipeline for OpenBeagle. Generated HTML is a CI artifact rather than checked-in content.

## Repository layout

| Path | Contents |
| --- | --- |
| `mainboard/` | Editable QuadESC mainboard design |
| `som/` | Editable AM13E module design |
| `mainboard_schematic.pdf`, `som_schematic.pdf` | Exported schematics |
| `documentation/source/` | Markdown manuals, shared diagrams and styling |
| `documentation/reference/` | Electrical connector, GPIO and analog reference data |
| `tools/documentation/` | Documentation build and validation scripts |
| `firmware/` | Firmware sources and existing application/loader artifacts |
| `drone-flasher-swd/` | SWD programming utility |

Documentation builds use only the manuals and their reference assets. See the [programming guide](documentation/source/som/firmware.md) for module programming interfaces.
