#!/usr/bin/env python3
"""Nominal QuadESC ADC-to-Celsius conversion using the VAA reference.

Assumes a 10k pull-up, R25=10k and B=3380 K. Confirm the fitted
thermistor's part and R/T curve before using this estimate for thermal control.
Usage: python3 thermistor-temperature.py 2048 1000
"""

import argparse
import math


def temperature_c(raw):
    """Convert an integer ADC count (1..4094) to approximate board °C."""
    if isinstance(raw, bool) or not isinstance(raw, (int, float)):
        raise ValueError("raw must be an integer count from 1 to 4094")
    if not 1 <= raw <= 4094 or not math.isfinite(raw) or int(raw) != raw:
        raise ValueError("raw must be an integer count from 1 to 4094; rail codes are invalid")
    resistance = 10000.0 * raw / (4096 - raw)
    return 1 / (1 / 298.15 + math.log(resistance / 10000) / 3380) - 273.15


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("raw", nargs="+", type=int, help="12-bit integer count, excluding rail codes")
    args = parser.parse_args()
    try:
        readings = [(raw, temperature_c(raw)) for raw in args.raw]
    except ValueError as error:
        parser.error(str(error))
    for raw, temperature in readings:
        print(f"raw {raw}: approximately {temperature:.2f} °C")


if __name__ == "__main__":
    main()
