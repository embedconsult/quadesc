#!/usr/bin/env bash
# Flash a raw AM13 MAIN image using the drone-flasher Jetson's SWD wiring.
# Usage: sudo ./flash-swd.sh IMAGE.bin BASE_ADDRESS
#        ./flash-swd.sh --check     # Checks OpenOCD/config; no GPIO/device access.
# Requires OpenOCD with BOTH linuxgpiod and am13 drivers; stock 0.11 lacks am13.
# Use OPENOCD=/absolute/path/to/openocd to select a suitable ARM64 build.
# The target must already be powered. This script does not control PSU or CAN.
set -euo pipefail
export LC_ALL=C
script_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
openocd=${OPENOCD:-openocd}
# Prefer the AM13-enabled build installed on this fixture.
if [[ -z ${OPENOCD:-} ]] && command -v openocd-am13 >/dev/null; then
    openocd=openocd-am13
fi
cfg="$script_dir/am13-jetson.cfg"
die() { echo "ERROR: $*" >&2; exit 1; }

# 'shutdown' exits during config parsing, before OpenOCD initializes hardware.
# Detect missing drivers before unbinding I2C or changing any Jetson pads.
preflight() {
    "$openocd" -f "$cfg" -c shutdown ||
        die 'OpenOCD must include linuxgpiod and the AM13 flash driver.'
}
if [[ ${1:-} == --check && $# == 1 ]]; then preflight; exit 0; fi
[[ $# == 2 ]] || die 'Usage: sudo ./flash-swd.sh IMAGE.bin BASE_ADDRESS'
[[ $EUID == 0 ]] || die 'Run on drone-flasher with sudo.'
[[ $(uname -m) == aarch64 ]] || die 'This pin mapping is for the ARM64 Jetson.'
[[ $(tr '\0' '\n' </proc/device-tree/compatible) == *tegra234* ]] ||
    die 'This pinmux mapping requires the Tegra234 Jetson.'
[[ $1 == *.bin && -f $1 && -r $1 ]] || die 'Supply a readable raw .bin file.'
[[ $2 =~ ^0x[0-9a-fA-F]{1,8}$ ]] || die 'Supply an explicit hexadecimal load address.'
base=$(( $2 )); size=$(stat -c %s -- "$1")
# AM13 erase sectors are 2 KiB. Round up the image tail and fill it with 0xff.
# Calibration occupies 0x78000..0x7ffff in this fixture; never touch it.
# An address of zero deliberately replaces the loader; use only a full image.
span=$(( (size + 2047) / 2048 * 2048 ))
(( size > 0 && base % 2048 == 0 && base + span <= 0x78000 )) ||
    die 'Image must start on a 2-KiB boundary and end before calibration at 0x78000.'
preflight

# Both fixture users and SWD users must be excluded for the whole transaction.
# A bench runner may pass its already-held fixture lock as descriptor 8.
if [[ $(readlink /proc/$$/fd/8 2>/dev/null || true) != /run/lock/drone-esc-som-fixture.lock ]]; then
    exec 8>/run/lock/drone-esc-som-fixture.lock
fi
flock -n 8 || die 'Fixture is busy.'
exec 9>/run/lock/som-swd.lock; flock -n 9 || die 'SWD is busy.'
controller=c250000.i2c
driver=/sys/bus/platform/drivers/tegra-i2c
compgen -G '/sys/bus/i2c/devices/7-*' >/dev/null && die 'I2C7 has registered clients.'
bound=0
if [[ -L /sys/bus/platform/devices/$controller/driver ]]; then
    [[ $(readlink -f "/sys/bus/platform/devices/$controller/driver") == "$driver" ]] ||
        die 'Unexpected I2C7 driver.'
    bound=1
fi
sda=$(busybox devmem 0x0c302018 32)
scl=$(busybox devmem 0x0c302020 32)
tmp=$(mktemp -d /run/am13-swd.XXXXXX)
pid=
cleanup() {
    local rc=$? failed=0
    trap - EXIT INT TERM
    # Stop OpenOCD and release its GPIO handles before restoring pinmux.
    if [[ -n $pid ]]; then
        kill -TERM "$pid" 2>/dev/null || true
        wait "$pid" 2>/dev/null || true
    fi
    # GPIO release can change SFSEL; restore both saved pad values afterwards.
    busybox devmem 0x0c302018 32 "$sda" || failed=1
    busybox devmem 0x0c302020 32 "$scl" || failed=1
    if (( bound )) && [[ ! -L /sys/bus/platform/devices/$controller/driver ]]; then
        printf '%s' "$controller" >"$driver/bind" || failed=1
    fi
    rm -rf -- "$tmp"
    (( failed == 0 )) || { echo 'ERROR: Pinmux/I2C restoration failed.' >&2; rc=1; }
    exit "$rc"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

# Copy to a fixed filename so arbitrary paths never enter OpenOCD's Tcl code.
cp -- "$1" "$tmp/image.bin"
python3 - "$tmp/image.bin" "$span" <<'PY'
import pathlib, sys
p = pathlib.Path(sys.argv[1])
b = p.read_bytes()
p.write_bytes(b + b'\xff' * (int(sys.argv[2]) - len(b)))
PY
echo "MAIN flash: address=$2 erase/program bytes=$span (input bytes=$size)"
sha256sum -- "$tmp/image.bin"

# Temporarily switch the native open-drain I2C pads into GPIO/SWD mode.
if (( bound )); then printf '%s' "$controller" >"$driver/unbind"; fi
busybox devmem 0x0c302018 32 "$((sda & ~0x400))"
busybox devmem 0x0c302020 32 "$((scl & ~0x400))"
cd -- "$tmp"
# Reset/halt; erase only covered sectors; program; compare the entire padded
# range; then reset/run. A command failure prevents the final reset/run.
# timeout bounds execution; no automatic retry or mass erase is performed.
timeout --signal=TERM --kill-after=3s 300s "$openocd" -f "$cfg" \
    -c "init; reset init; flash write_image erase image.bin $2 bin; verify_image image.bin $2 bin; reset run; shutdown" &
pid=$!
wait "$pid"
pid=
echo 'Flash verified; target reset to run. Restoring Jetson pinmux/I2C.'
