# Firmware sources

This directory contains the board application and its supporting libraries in `source/`, existing application artifacts in `application/`, and a separate loader with its own source tree in `loader/`.

The application and loader source trees are independent. Build instructions and configuration belong to their respective source projects.

Hardware interfaces and general programming access are documented in the [AM13E SoM programming guide](../documentation/source/som/firmware.md) and [QuadESC manual](../documentation/source/quadesc/index.md). The documentation CI builds the hardware manuals independently of these firmware trees.
