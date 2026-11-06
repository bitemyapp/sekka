# SPDX-License-Identifier: GPL-3.0-or-later
"""Load the pinned Asahi installer's modules and see the Mac through them.

The modules come from the extracted installer (Asahi v0.9.2's tarball,
pinned by SHA-256), never from a copy in Sekka's tree. Each module's
`subprocess` is replaced with the engine's SubprocessShim, so the commands
Asahi runs go through the engine's Host.
"""

import importlib
import logging
import os
import sys
import types

from . import ASAHI_TAG
from .host import SubprocessShim

log = logging.getLogger("sekka_engine.asahi")

MODULES = ("util", "system", "diskutil", "osenum", "m1n1", "urlcache", "osinstall", "stub", "bugs", "main")
FIRMWARE_MODULES = ("asahi_firmware", "asahi_firmware.core")


class AsahiError(RuntimeError):
    pass


def load(root, host):
    """Import Asahi's modules from `root` and route their commands through
    `host`. Returns a namespace with each module by name."""
    root = os.path.abspath(root)
    try:
        with open(os.path.join(root, "version.tag")) as f:
            tag = f.read().strip()
    except OSError as error:
        raise AsahiError(f"the Asahi installer isn't at {root}") from error
    if tag != ASAHI_TAG:
        raise AsahiError(f"the Asahi installer is {tag}, not {ASAHI_TAG}")
    if root not in sys.path:
        sys.path.append(root)
    modules = {}
    for name in MODULES + FIRMWARE_MODULES:
        module = importlib.import_module(name)
        origin = os.path.abspath(getattr(module, "__file__", "") or "")
        if not origin.startswith(root + os.sep):
            raise AsahiError(f"{name} was imported from {origin}, not the pinned installer")
        modules[name.replace(".", "_")] = module
    shim = SubprocessShim(host)
    for module in list(sys.modules.values()):
        origin = os.path.abspath(getattr(module, "__file__", "") or "")
        if origin.startswith(root + os.sep) and hasattr(module, "subprocess"):
            module.subprocess = shim
    ns = types.SimpleNamespace(root=root, shim=shim, host=host, **modules)
    return ns


def read_only_diskutil(asahi):
    """Asahi's DiskUtil, mounting read-only and remembering what it mounted,
    for inspection: looking must not change what is mounted, or how."""

    class ReadOnlyDiskUtil(asahi.diskutil.DiskUtil):
        def __init__(self):
            super().__init__()
            self.mounted_here = []

        def mount(self, target):
            before = self.get("info", "-plist", target).get("MountPoint") or ""
            if not before:
                self.action("mount", "readOnly", target)
                self.mounted_here.append(target)
            info = self.get("info", "-plist", target)
            return info["MountPoint"]

        def restore_mounts(self):
            for target in reversed(self.mounted_here):
                try:
                    self.action("unmount", target)
                except Exception as error:  # report, don't hide the inspection
                    log.warning("couldn't unmount %s again: %s", target, error)
            self.mounted_here = []

    return ReadOnlyDiskUtil()


class View:
    """The Mac as Asahi's own model describes it: SystemInfo (ioreg, nvram,
    bputil, the SystemVersion plists), DiskUtil (diskutil's plists) and
    OSEnum (each container's operating systems)."""

    def __init__(self, asahi, read_only):
        self.asahi = asahi
        self.read_only = read_only
        self.sysinfo = asahi.system.SystemInfo()
        self.dutil = None
        self.refresh()

    def refresh(self):
        if self.dutil is not None and self.read_only:
            self.dutil.restore_mounts()
        a = self.asahi
        self.dutil = read_only_diskutil(a) if self.read_only else a.diskutil.DiskUtil()
        self.dutil.get_info()
        self.sys_disk = self.dutil.find_system_disk()
        self.parts = self.dutil.get_partitions(self.sys_disk)
        self.osinfo = a.osenum.OSEnum(self.sysinfo, self.dutil, self.sys_disk)
        self.osinfo.collect(self.parts)
        for i, p in enumerate(self.parts):
            p.index = i
        return self

    def close(self):
        if self.dutil is not None and self.read_only:
            self.dutil.restore_mounts()

    # Lookups by identity: device names (disk0s3) change as partitions are
    # added, so nothing is remembered by name between refreshes.

    def partition(self, gpt_uuid):
        want = gpt_uuid.lower()
        for p in self.parts:
            if not p.free and (p.uuid or "").lower() == want:
                return p
        return None

    def oses(self):
        for p in self.parts:
            for o in p.os or []:
                yield p, o

    def os_by_group(self, vgid):
        want = vgid.lower()
        for p, o in self.oses():
            if (o.vgid or "").lower() == want:
                return p, o
        return None, None

    def current_os(self):
        """The macOS this engine runs on, as Asahi's main_loop finds it: the
        OS whose volume group is the booted one."""
        for p, o in self.oses():
            if o.version and self.sysinfo.boot_uuid and o.vgid == self.sysinfo.boot_uuid:
                return o
        return None
