#!/usr/bin/env python3
"""Enable wake along a USB input device's path, including hubs/controllers.

Called by udev only for keyboard/mouse input events. No autosuspend policy is
changed, and unrelated USB devices (cameras, storage, etc.) are not enabled.
"""
from pathlib import Path
import sys


def enable(device: Path, sysfs: Path = Path('/sys')) -> list[Path]:
    device = device.resolve(strict=True)
    devices = (sysfs / 'devices').resolve(strict=True)
    if not device.is_relative_to(devices):
        raise ValueError('Input device is outside sysfs devices')
    parents = []
    while device != devices:
        parents.append(device)
        device = device.parent
    # Require a USB device ancestor even if a misleading udev event calls us.
    if not any((p / 'idVendor').is_file() and (p / 'idProduct').is_file() for p in parents):
        return []
    changed = []
    for parent in parents:
        wake = parent / 'power/wakeup'
        try:
            value = wake.read_text().strip()
            if value == 'disabled':
                wake.write_text('enabled\n')
                changed.append(wake)
        except FileNotFoundError:
            pass  # Devices without wake support and hot-unplug races.
    return changed


if __name__ == '__main__':
    if len(sys.argv) != 2:
        raise SystemExit('Usage: sekka-usb-wake /sys/devices/.../input/eventN')
    try:
        enable(Path(sys.argv[1]))
    except FileNotFoundError:
        pass  # The device was unplugged before udev ran its queued action.
