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


class Mounts:
    """What one run (or one inspection) mounted, by identity, and how.

    `writable(info, dutil)` says which volumes may be mounted read-write
    (by default none); everything else is mounted read-only. Volumes that
    were already mounted when asked for are left as they are, except that a
    volume this ledger mounted read-only is mounted again read-write once it
    becomes writable. `restore` unmounts everything this ledger mounted.
    Records are kept by GPT or volume UUID, since device names change as
    partitions are added."""

    def __init__(self, writable=None):
        self.writable = writable or (lambda info, dutil: False)
        self.records = {}  # identity -> "ro" | "rw"

    def identity(self, info, target):
        return (info.get("DiskUUID") or info.get("VolumeUUID") or target).upper()

    def restore(self, dutil):
        for identity in reversed(list(self.records)):
            try:
                dutil.action("unmount", identity)
            except Exception as error:  # report it, don't hide the run's result
                log.warning("couldn't unmount %s again: %s", identity, error)
        self.records = {}


def policy_diskutil(asahi, mounts):
    """Asahi's DiskUtil whose mount() follows a Mounts ledger: read-write only
    where the ledger allows, read-only otherwise, and every mount recorded so
    that it can be undone. Asahi's own code (OSEnum, the stub install) mounts
    through it too."""

    class PolicyDiskUtil(asahi.diskutil.DiskUtil):
        def mount(self, target):
            info = self.get("info", "-plist", target)
            identity = mounts.identity(info, target)
            want = "rw" if mounts.writable(info, self) else "ro"
            have = mounts.records.get(identity)
            if info.get("MountPoint"):
                if have != "ro" or want == "ro":
                    return info["MountPoint"]
                # Ours, read-only, and now Sekka's own: mount it writable.
                self.action("unmount", target)
            if want == "rw":
                self.action("mount", target)
            else:
                self.action("mount", "readOnly", target)
            mounts.records[identity] = want
            return self.get("info", "-plist", target)["MountPoint"]

        def mount_read_only(self, target):
            """Mounted read-only by this ledger, whoever mounted it before."""
            info = self.get("info", "-plist", target)
            identity = mounts.identity(info, target)
            if info.get("MountPoint"):
                if mounts.records.get(identity) == "ro":
                    return info["MountPoint"]
                self.action("unmount", target)
            self.action("mount", "readOnly", target)
            mounts.records[identity] = "ro"
            return self.get("info", "-plist", target)["MountPoint"]

        def unmount(self, target):
            info = self.get("info", "-plist", target)
            self.action("unmount", target)
            mounts.records.pop(mounts.identity(info, target), None)

        def restore_mounts(self):
            mounts.restore(self)

    return PolicyDiskUtil()


class View:
    """The Mac as Asahi's own model describes it: SystemInfo (ioreg, nvram,
    bputil, the SystemVersion plists), DiskUtil (diskutil's plists) and
    OSEnum (each container's operating systems).

    Mounts follow the given ledger; without one, everything is mounted
    read-only. close() unmounts what the view mounted."""

    def __init__(self, asahi, mounts=None):
        self.asahi = asahi
        self.mounts = mounts if mounts is not None else Mounts()
        self.sysinfo = asahi.system.SystemInfo()
        self.dutil = None
        self.refresh()

    def refresh(self):
        a = self.asahi
        self.dutil = policy_diskutil(a, self.mounts)
        self.dutil.get_info()
        self.sys_disk = self.dutil.find_system_disk()
        self.parts = self.dutil.get_partitions(self.sys_disk)
        self.osinfo = a.osenum.OSEnum(self.sysinfo, self.dutil, self.sys_disk)
        self.osinfo.collect(self.parts)
        for i, p in enumerate(self.parts):
            p.index = i
        return self

    def close(self):
        if self.dutil is not None:
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
