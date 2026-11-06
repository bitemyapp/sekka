# SPDX-License-Identifier: GPL-3.0-or-later
"""A fake Mac for the engine's tests.

FakeMac keeps one model of a Mac (its disk's partitions, APFS containers and
volumes, device tree, NVRAM, boot policies, power and FileVault) and answers
the commands Asahi's modules and the adapter run from it: diskutil's plists,
ioreg's trees, bputil, bless, newfs_msdos and the rest. Volumes and
partitions are directories and files under a temporary root; nothing here
touches a real disk, and nothing reaches the network.

The engine sees the fake only through the Host it is given and through the
test-only patches in `asahi_patches` (libSystem calls and Apple's LZFSE
compression, which Linux lacks). Neither exists in a real run.
"""

import contextlib
import hashlib
import io
import json
import os
import plistlib
import random
import shutil
import struct
import sys
import tempfile
import uuid as uuidlib
import warnings
import zipfile
from unittest import mock

HERE = os.path.dirname(os.path.abspath(__file__))
ENGINE = os.path.dirname(HERE)
MACOS = os.path.dirname(ENGINE)
sys.path.insert(0, ENGINE)

from sekka_engine import asahi as asahi_loader  # noqa: E402
from sekka_engine import canonical  # noqa: E402
from sekka_engine import contract as c  # noqa: E402
from sekka_engine import host as host_module  # noqa: E402

GIB = 1 << 30
MIB = 1 << 20
PROFILE_PATH = os.path.join(MACOS, "crates", "contract", "profiles", "j514s.json")
FIXTURES = os.path.join(HERE, "fixtures")
VECTORS = os.path.join(HERE, "vectors")
OWNER = "callen"
PASSWORD = b"correct horse battery staple"
NOW = 1_800_000_000
# The tests' Hello names a fixed adapter digest, so that transcripts don't
# change with every edit to the adapter; test_cli checks the real one.
ADAPTER = canonical.digest_bytes(b"sekka_engine under test")


def asahi_root():
    """The extracted pinned Asahi installer (v0.9.2's tarball). The test
    runner names it in SEKKA_ASAHI_ROOT; the engine itself never reads that."""
    root = os.environ.get("SEKKA_ASAHI_ROOT")
    if not root or not os.path.isfile(os.path.join(root, "version.tag")):
        raise RuntimeError("set SEKKA_ASAHI_ROOT to the extracted Asahi installer v0.9.2")
    return root


def profile_bytes():
    with open(PROFILE_PATH, "rb") as f:
        return f.read()


def plist(value):
    return plistlib.dumps(value, fmt=plistlib.FMT_XML)


class Volume:
    def __init__(self, dev, vuuid, name, roles, filevault=False):
        self.dev = dev
        self.uuid = vuuid
        self.name = name
        self.roles = list(roles)
        self.filevault = filevault
        self.group = None
        self.snapshots = []
        self.mounted = False
        self.dir = None


class Container:
    def __init__(self, ref, cuuid, ceiling, free):
        self.ref = ref
        self.uuid = cuuid
        self.ceiling = ceiling
        self.free = free
        self.volumes = []
        self.next_vol = 1


class Part:
    def __init__(self, puuid, content, offset, size, gpt_name="", volume_name=""):
        self.uuid = puuid
        self.content = content
        self.offset = offset
        self.size = size
        self.gpt_name = gpt_name
        self.volume_name = volume_name
        self.container = None
        self.fat = content == "EFI"
        self.mounted = False
        self.dir = None


class FakeMac:
    def __init__(self, root, seed=1, privileged=True):
        self.root = root
        self.rng = random.Random(seed)
        self.privileged = privileged
        self.disk_size = 500277792768
        self.block = 4096
        self.disk_uuid = self.new_uuid()
        self.parts = []
        self.containers = {}
        self.next_ref = 1
        self.platform_uuid = self.new_uuid()
        self.device_class = "j514sap"
        self.product_type = "Mac15,6"
        self.board_id = 0x44
        self.chip_id = 0x6030
        self.sys_firmware = "mBoot-20457.1.29"
        self.product_name = "MacBook Pro (14-inch, Nov 2023)"
        self.soc_name = "Apple M3 Pro"
        self.macos = ("27.0.1", "26A434")
        self.sfr = ("27.0.1", "26A434", "27.0.434.0.0,0")
        self.boot_group = None
        self.nvram_boot_group = None
        self.policies = {}
        self.pmset = "Now drawing from 'AC Power'\n -InternalBattery-0 (id=1234)\t80%; charged; 0:00 remaining present: true\n"
        self.fdesetup = "FileVault is On.\n"
        self.computer_name = "Chris's MacBook Pro"
        self.owner_password = PASSWORD
        self.limits = {}
        self.resize_failure = None
        self.bless_mode = "ok"
        self.commands = []
        self.extra_internal_disks = 0
        os.makedirs(os.path.join(root, "mnt"), exist_ok=True)
        os.makedirs(os.path.join(root, "raw"), exist_ok=True)

    # Model helpers ---------------------------------------------------------------

    def new_uuid(self):
        return str(uuidlib.UUID(int=self.rng.getrandbits(128), version=4)).upper()

    def ordered(self):
        return sorted(self.parts, key=lambda p: p.offset)

    def part_name(self, p):
        return f"disk0s{self.ordered().index(p) + 1}"

    def part_by_name(self, name):
        name = name.removeprefix("/dev/").removeprefix("r")
        for p in self.ordered():
            if self.part_name(p) == name:
                return p
        return None

    def add_part(self, content, offset, size, gpt_name="", volume_name="", puuid=None):
        p = Part(puuid or self.new_uuid(), content, offset, size, gpt_name, volume_name)
        p.dir = os.path.join(self.root, "mnt", p.uuid)
        os.makedirs(p.dir, exist_ok=True)
        self.parts.append(p)
        return p

    def add_container(self, part, used, cuuid=None, ref=None):
        if ref is None:
            ref = f"disk{self.next_ref + 1}"
            self.next_ref += 1
        ct = Container(ref, cuuid or self.new_uuid(), part.size, part.size - used)
        part.container = ct
        self.containers[ref] = ct
        return ct

    def add_volume(self, ct, name, roles, vuuid=None, filevault=False, dev=None):
        dev = dev or f"{ct.ref}s{ct.next_vol}"
        ct.next_vol += 1
        v = Volume(dev, vuuid or self.new_uuid(), name, roles, filevault)
        v.dir = os.path.join(self.root, "mnt", v.uuid)
        os.makedirs(v.dir, exist_ok=True)
        ct.volumes.append(v)
        return v

    def group(self, data, system, guuid=None):
        g = guuid or data.uuid
        data.group = system.group = g
        return g

    def volume_by_dev(self, dev):
        dev = dev.removeprefix("/dev/")
        for ct in self.containers.values():
            for v in ct.volumes:
                if v.dev == dev or dev == v.dev + "s1" and "System" in v.roles:
                    return ct, v
        return None, None

    def store_part(self, ct):
        for p in self.parts:
            if p.container is ct:
                return p
        return None

    def macos_system(self):
        for ct in self.containers.values():
            for v in ct.volumes:
                if v.group == self.boot_group and "System" in v.roles:
                    return v
        raise AssertionError("no macOS system volume")

    def fs(self, path):
        """An absolute macOS path, inside the running macOS's System volume."""
        if not path.startswith("/"):
            return path
        return os.path.join(self.macos_system().dir, path.lstrip("/"))

    def raw_path(self, p):
        return os.path.join(self.root, "raw", p.uuid + ".img")

    # Answers to commands ---------------------------------------------------------------

    def run(self, argv, input=None):
        name = argv[0]
        if name not in host_module.COMMANDS:
            raise AssertionError(f"the engine ran {name}, which isn't on its list")
        self.commands.append(list(argv))
        handler = getattr(self, "cmd_" + name, None)
        if handler is None:
            raise AssertionError(f"unexpected command {argv}")
        result = handler(argv[1:], input)
        if isinstance(result, tuple):
            rc, out, err = result
        else:
            rc, out, err = 0, result, b""
        if isinstance(out, str):
            out = out.encode()
        if isinstance(err, str):
            err = err.encode()
        return host_module.Completed(argv, rc, out, err)

    def cmd_ioreg(self, args, input):
        if args == ["-alp", "IODeviceTree"]:
            return plist(self.device_tree())
        if args == ["-alp", "IOService"]:
            return plist(self.io_service())
        raise AssertionError(args)

    def device_tree(self):
        compatible = f"{self.device_class[:-2].upper()}{self.device_class[-2:].upper()}\0{self.product_type}\0AppleARM\0"
        compatible = compatible.encode()
        chosen = {
            "IORegistryEntryName": "chosen",
            "board-id": struct.pack("<I", self.board_id),
            "chip-id": struct.pack("<I", self.chip_id),
            "system-firmware-version": self.sys_firmware.encode() + b"\0",
            "boot-uuid": self.boot_group.encode() + b"\0",
            "associated-volume-group": self.boot_group.encode() + b"\0",
        }
        product = {
            "IORegistryEntryName": "product",
            "product-name": self.product_name.encode() + b"\0",
            "product-soc-name": self.soc_name.encode() + b"\0",
        }
        return {
            "IORegistryEntryChildren": [
                {
                    "IOObjectClass": "IOPlatformExpertDevice",
                    "IORegistryEntryName": "device-tree",
                    "compatible": compatible,
                    "IORegistryEntryChildren": [chosen, product],
                }
            ]
        }

    def io_service(self):
        partitions = [
            {
                "IOObjectClass": "IOMedia",
                "BSD Name": self.part_name(p),
                "IORegistryEntryName": p.gpt_name,
                "UUID": p.uuid,
                "Base": p.offset,
                "Size": p.size,
                "Whole": False,
            }
            for p in self.ordered()
        ]
        disk = {
            "IOObjectClass": "IOMedia",
            "BSD Name": "disk0",
            "Whole": True,
            "Size": self.disk_size,
            "IORegistryEntryChildren": [
                {"IOObjectClass": "IOMediaBSDClient"},
                {"IOObjectClass": "IOGUIDPartitionScheme", "UUID": self.disk_uuid, "IORegistryEntryChildren": partitions},
            ],
        }
        return {
            "IORegistryEntryChildren": [
                {
                    "IOObjectClass": "IOPlatformExpertDevice",
                    "IOPlatformUUID": self.platform_uuid,
                    "IORegistryEntryChildren": [disk],
                }
            ]
        }

    def cmd_bputil(self, args, input):
        if not self.privileged:
            return 1, b"", b"bputil: this tool must be run as root\n"
        if args[:2] != ["-d", "-v"] or len(args) != 3:
            raise AssertionError(args)
        vgid = args[2].upper()
        if vgid == (self.boot_group or "").upper():
            return "Current OS environment: macOS\nOS Type                                       : macOS\n"
        if vgid not in self.policies:
            return 1, b"", b"bputil: no policy for that volume group\n"
        coih = self.policies[vgid] or "absent"
        return (
            "OS Type                                       : macOS\n"
            "Local Policy Nonce Hash (lpnh): 1111\n"
            "Next Stage Image4 Hash (nsih): 2222\n"
            f"Custom Kernel Collection Hash (coih): {coih}\n"
        )

    def cmd_nvram(self, args, input):
        assert args == ["-p"], args
        lines = ["auto-boot\ttrue"]
        if self.nvram_boot_group:
            lines.append(f"boot-volume\tEF57347C-0000-AA11-AA11-00306543ECAC:{self.disk_uuid}:{self.nvram_boot_group}")
        return "\n".join(lines) + "\n"

    def cmd_scutil(self, args, input):
        if args == ["--get", "ComputerName"]:
            return self.computer_name + "\n"
        if not args and input == b"show State:/Users/ConsoleUser\n":
            return "<dictionary> {\n  kCGSSessionUserNameKey : callen\n}\n"
        raise AssertionError((args, input))

    def cmd_pmset(self, args, input):
        assert args == ["-g", "batt"]
        return self.pmset

    def cmd_fdesetup(self, args, input):
        assert args == ["status"]
        return self.fdesetup

    def cmd_sync(self, args, input):
        return b""

    def cmd_xattr(self, args, input):
        return b""

    def cmd_chflags(self, args, input):
        return b""

    def cmd_mount(self, args, input):
        assert args[:2] == ["-u", "-w"], args
        return b""

    def cmd_hdiutil(self, args, input):
        if args[0] == "attach":
            mountpoint = args[args.index("-mountpoint") + 1]
            os.makedirs(mountpoint, exist_ok=True)
            return b""
        if args[0] == "detach":
            return b""
        raise AssertionError(args)

    def cmd_tar(self, args, input):
        assert args[:2] == ["czf", "all_firmware.tar.gz"], args
        with open("all_firmware.tar.gz", "wb") as f:
            f.write(b"fake firmware archive\n")
        return b""

    def cmd_newfs_msdos(self, args, input):
        p = self.part_by_name(args[-1])
        assert p is not None and p.content == "EFI", args
        label = args[args.index("-v") + 1]
        p.fat = True
        p.volume_name = label.upper()
        shutil.rmtree(p.dir)
        os.makedirs(p.dir)
        return b""

    def cmd_bless(self, args, input):
        if args == ["--getBoot"]:
            for ct in self.containers.values():
                for v in ct.volumes:
                    if v.group == self.nvram_boot_group and "System" in v.roles:
                        return f"/dev/{v.dev}\n"
            return 1, b"", b"no boot volume\n"
        assert args[0] == "--setBoot" and args[1] == "--device" and "--stdinpass" in args, args
        if self.bless_mode == "fail":
            return 3, b"", b"Could not set boot device property: 0xe00002bc\n"
        ct, v = self.volume_by_dev(args[2])
        assert v is not None and args[args.index("--user") + 1] == OWNER, args
        if input != self.owner_password:
            return 0, b"", b""  # bless is known to exit 0 when it refuses the password
        if self.bless_mode == "ok":
            self.nvram_boot_group = v.group
        return b""

    # diskutil ---------------------------------------------------------------

    def cmd_diskutil(self, args, input):
        if args == ["list", "-plist"]:
            return plist(self.list_plist())
        if args[:2] == ["apfs", "list"]:
            refs = self.select_containers(args[2] if len(args) == 4 else None)
            return plist({"Containers": [self.apfs_container(ct) for ct in refs]})
        if args[:2] == ["apfs", "listVolumeGroups"]:
            ct = self.select_containers(args[2])[0]
            return plist({"Containers": [self.volume_groups(ct)]})
        if args[:2] == ["info", "-plist"]:
            info = self.info(args[2])
            if info is None:
                return 1, b"", f"Could not find disk: {args[2]}\n"
            return plist(info)
        if args[:2] == ["apfs", "resizeContainer"]:
            return self.resize(args[2:])
        if args[:2] == ["apfs", "listSnapshots"]:
            ct, v = self.volume_by_dev(args[2])
            if v is None:
                return 1, b"", b"not a volume\n"
            return plist({"Snapshots": list(v.snapshots)})
        if args[0] == "mount":
            target = args[-1]
            p = self.part_by_name(target)
            if p is not None:
                if not p.fat:
                    return 1, b"", b"Volume on disk0 failed to mount\n"
                p.mounted = True
                return f"Volume {p.volume_name} on {target} mounted\n"
            ct, v = self.volume_by_dev(target)
            if v is None:
                return 1, b"", b"no such volume\n"
            v.mounted = True
            return f"Volume {v.name} on {target} mounted\n"
        if args[0] == "unmount":
            p = self.part_by_name(args[1])
            if p is not None:
                p.mounted = False
            else:
                ct, v = self.volume_by_dev(args[1])
                v.mounted = False
            return b"unmounted\n"
        if args[0] == "addPartition":
            return self.add_partition(*args[1:])
        if args[:2] == ["apfs", "addVolume"]:
            return self.apfs_add_volume(args[2:])
        if args[:2] == ["apfs", "changeVolumeRole"]:
            ct, v = self.volume_by_dev(args[2])
            v.roles = {"D": ["Data"], "S": ["System"]}[args[3]]
            return b""
        if args[0] == "rename":
            ct, v = self.volume_by_dev(args[1])
            v.name = args[2]
            return b""
        raise AssertionError(f"unexpected diskutil {args}")

    def select_containers(self, ref):
        if ref is None:
            return list(self.containers.values())
        if ref in self.containers:
            return [self.containers[ref]]
        p = self.part_by_name(ref)
        assert p is not None and p.container is not None, ref
        return [p.container]

    def list_plist(self):
        parts = []
        for p in self.ordered():
            entry = {"Content": p.content, "DeviceIdentifier": self.part_name(p), "DiskUUID": p.uuid, "Size": p.size}
            if p.fat and p.volume_name:
                entry["VolumeName"] = p.volume_name
            parts.append(entry)
        disks = [{"Content": "GUID_partition_scheme", "DeviceIdentifier": "disk0", "OSInternal": False, "Partitions": parts, "Size": self.disk_size}]
        whole = ["disk0"]
        for i in range(self.extra_internal_disks):
            name = f"disk{90 + i}"
            disks.append({"Content": "GUID_partition_scheme", "DeviceIdentifier": name, "Partitions": [{"Content": "Apple_APFS_ISC", "DeviceIdentifier": name + "s1", "Size": 1}], "Size": 1})
            whole.append(name)
        for ct in self.containers.values():
            vols = []
            for v in ct.volumes:
                entry = {"DeviceIdentifier": v.dev, "DiskUUID": v.uuid, "VolumeName": v.name}
                if "System" in v.roles and v.group == self.boot_group:
                    entry["MountedSnapshots"] = [{"SnapshotBSD": v.dev + "s1", "SnapshotName": "com.apple.os.update-1", "SnapshotMountPoint": "/"}]
                vols.append(entry)
            store = self.store_part(ct)
            disks.append(
                {
                    "APFSPhysicalStores": [{"DeviceIdentifier": self.part_name(store)}],
                    "APFSVolumes": vols,
                    "Content": "",
                    "DeviceIdentifier": ct.ref,
                    "OSInternal": False,
                    "Size": ct.ceiling,
                }
            )
            whole.append(ct.ref)
        return {"AllDisks": whole, "AllDisksAndPartitions": disks, "VolumesFromDisks": [], "WholeDisks": whole}

    def apfs_container(self, ct):
        store = self.store_part(ct)
        return {
            "APFSContainerUUID": ct.uuid,
            "CapacityCeiling": ct.ceiling,
            "CapacityFree": ct.free,
            "ContainerReference": ct.ref,
            "DesignatedPhysicalStore": self.part_name(store),
            "PhysicalStores": [{"DeviceIdentifier": self.part_name(store), "DiskUUID": store.uuid, "Size": store.size}],
            "Volumes": [
                {
                    "APFSVolumeUUID": v.uuid,
                    "CapacityInUse": 1,
                    "CryptoMigrationOn": False,
                    "DeviceIdentifier": v.dev,
                    "Encryption": v.filevault,
                    "FileVault": v.filevault,
                    "Locked": False,
                    "Name": v.name,
                    "Roles": list(v.roles),
                }
                for v in ct.volumes
            ],
        }

    def volume_groups(self, ct):
        groups = {}
        for v in ct.volumes:
            if v.group and ("Data" in v.roles or "System" in v.roles):
                groups.setdefault(v.group, []).append(
                    {"DeviceIdentifier": v.dev, "DiskUUID": v.uuid, "NoBrowseRole": False, "Role": v.roles[0]}
                )
        return {
            "APFSContainerUUID": ct.uuid,
            "ContainerReference": ct.ref,
            "VolumeGroups": [{"APFSVolumeGroupUUID": g, "Volumes": vols} for g, vols in groups.items()],
        }

    def info(self, target):
        target = target.removeprefix("/dev/")
        if target == "disk0":
            return {
                "Content": "GUID_partition_scheme",
                "DeviceBlockSize": self.block,
                "DeviceIdentifier": "disk0",
                "Internal": True,
                "MountPoint": "",
                "Size": self.disk_size,
                "VirtualOrPhysical": "Unknown",
                "WholeDisk": True,
            }
        if target.startswith("disk9") and "s" not in target[4:]:
            return {"DeviceBlockSize": 4096, "DeviceIdentifier": target, "Internal": True, "VirtualOrPhysical": "Physical", "WholeDisk": True, "Size": 1}
        if target in self.containers:
            ct = self.containers[target]
            return {"DeviceIdentifier": target, "Internal": True, "VirtualOrPhysical": "Virtual", "WholeDisk": True, "Size": ct.ceiling, "Content": ""}
        p = self.part_by_name(target)
        if p is not None:
            info = {
                "Content": p.content,
                "DeviceBlockSize": self.block,
                "DeviceIdentifier": self.part_name(p),
                "DeviceNode": "/dev/" + self.part_name(p),
                "DiskUUID": p.uuid,
                "IORegistryEntryName": p.gpt_name,
                "Internal": True,
                "MountPoint": p.dir if p.mounted else "",
                "ParentWholeDisk": "disk0",
                "PartitionMapPartition": True,
                "PartitionMapPartitionOffset": p.offset,
                "Size": p.size,
                "VolumeName": p.volume_name if p.fat else "",
                "WholeDisk": False,
            }
            if p.container is not None:
                info["APFSContainerReference"] = p.container.ref
            return info
        ct, v = self.volume_by_dev(target)
        if v is not None:
            return {
                "APFSContainerReference": ct.ref,
                "APFSVolumeGroupID": v.group or "",
                "DeviceIdentifier": target,
                "DiskUUID": v.uuid,
                "MountPoint": v.dir if v.mounted else "",
                "VolumeName": v.name,
                "WholeDisk": False,
            }
        return None

    def resize(self, args):
        p = self.part_by_name(args[0])
        assert p is not None and p.container is not None, args
        if args[1:] == ["limits", "-plist"]:
            limits = self.limits.get(p.uuid)
            if limits is None:
                return 1, b"", b"Error: unable to determine limits\n"
            return plist(limits)
        size = int(args[1])
        if self.resize_failure is not None:
            rc, output = self.resize_failure
            return rc, output, b""
        used = p.container.ceiling - p.container.free
        p.size = size
        p.container.ceiling = size
        p.container.free = size - used
        return "Started APFS operation\nResizing APFS Physical Store\nFinished APFS operation\n"

    def add_partition(self, after, fs, label, size):
        size = int(size)
        prev = self.part_by_name(after)
        assert prev is not None, after
        start = prev.offset + prev.size
        nxt = [p for p in self.ordered() if p.offset >= start]
        limit = nxt[0].offset if nxt else (self.disk_size - 34 * 512) // 4096 * 4096
        if start + size > limit:
            return 1, b"Error: -69519: The target disk is too small for this operation\n", b""
        content = {"apfs": "Apple_APFS", "%EFI%": "EFI", "%Linux%": "Linux Filesystem"}[fs]
        p = self.add_part(content, start, size, gpt_name="Untitled")
        if content == "EFI":
            p.fat = False
        if fs == "apfs":
            ct = self.add_container(p, used=1 * MIB)
            self.add_volume(ct, label, [])
        return "Started partitioning on disk0\nFinished partitioning on disk0\n"

    def apfs_add_volume(self, args):
        ref, fs, name = args[0], args[1], args[2]
        assert fs == "apfs"
        ct = self.containers[ref]
        opts = dict(zip(args[3::2], args[4::2]))
        role = {"S": "System", "D": "Data", "B": "Preboot", "R": "Recovery"}[opts["-role"]]
        v = self.add_volume(ct, name, [role])
        if "-groupWith" in opts:
            _, data = self.volume_by_dev(opts["-groupWith"])
            self.group(data, v)
        return b""

    # Observing the fake -------------------------------------------------------------

    def state_digest(self):
        """Everything the engine could change on the Mac: the partition
        table, the containers and volumes, every file on them, the raw
        partitions and the startup selection."""
        h = hashlib.sha256()
        for p in self.ordered():
            h.update(repr((p.uuid, p.content, p.offset, p.size, p.fat, p.volume_name)).encode())
            ct = p.container
            if ct is not None:
                h.update(repr((ct.uuid, ct.ceiling)).encode())
                for v in ct.volumes:
                    h.update(repr((v.uuid, v.name, v.roles, v.group)).encode())
        h.update(repr(self.nvram_boot_group).encode())
        for base in (os.path.join(self.root, "mnt"), os.path.join(self.root, "raw")):
            for dirpath, dirnames, filenames in sorted(os.walk(base)):
                dirnames.sort()
                for name in sorted(filenames + [d for d in dirnames]):
                    path = os.path.join(dirpath, name)
                    h.update(os.path.relpath(path, base).encode())
                    if os.path.islink(path):
                        h.update(os.readlink(path).encode())
                    elif os.path.isfile(path):
                        with open(path, "rb") as f:
                            h.update(f.read())
        return h.hexdigest()


class FakeHost(host_module.Host):
    def __init__(self, mac):
        self.mac = mac
        self.corrupt_read_back = False

    def run(self, argv, input=None, timeout=None):
        return self.mac.run([str(a) for a in argv], input=input)

    def open_raw(self, device, writable):
        p = self.mac.part_by_name(device)
        assert p is not None, device
        path = self.mac.raw_path(p)
        if writable:
            assert p.content == "Linux Filesystem", "only the root partition is written raw"
            if not os.path.exists(path):
                open(path, "wb").close()
            return open(path, "r+b", buffering=0)
        f = open(path, "rb", buffering=0)
        if self.corrupt_read_back:
            data = bytearray(f.read())
            f.close()
            data[len(data) // 2] ^= 0xFF
            return io.BytesIO(bytes(data))
        return f

    def flush_device(self, fileobj):
        os.fsync(fileobj.fileno())
        return "fsync"

    def read_file(self, path):
        with open(self.mac.fs(path), "rb") as f:
            return f.read()

    def readlink(self, path):
        return os.readlink(self.mac.fs(path))

    def now(self):
        return NOW


# Scenarios -----------------------------------------------------------------------------

# The reference J514S's disk, from its GPT (sysfs start and size, in bytes).
ISC = (24576, 524288000)
CONTAINER_OFFSET = 524312576
CONTAINER_CLEAN = 494384795648
CONTAINER_AFTER = 290698821632
RECOVERY = (494909108224, 5368664064)
STUB = (291223134208, 2499805184)
ESP = (293722939392, 1073741824)
ROOT = (294796681216, 200112340992)
USED = 150_000_000_000
PREFERRED = 186_500_000_000


def base_mac(root, container_size=CONTAINER_CLEAN, privileged=True, seed=1):
    """A J514S running macOS 27.0.1 from its internal disk."""
    mac = FakeMac(root, seed=seed, privileged=privileged)
    isc = mac.add_part("Apple_APFS_ISC", *ISC, gpt_name="iBootSystemContainer")
    ct = mac.add_container(isc, used=20 * MIB)
    mac.add_volume(ct, "iSCPreboot", ["Preboot"])
    mac.add_volume(ct, "xART", ["xART"])
    mac.add_volume(ct, "Hardware", ["Hardware"])
    mac.add_volume(ct, "Recovery", ["Recovery"])
    macos = mac.add_part("Apple_APFS", CONTAINER_OFFSET, container_size, gpt_name="Container")
    ct = mac.add_container(macos, used=USED)
    system = mac.add_volume(ct, "Macintosh HD", ["System"])
    preboot = mac.add_volume(ct, "Preboot", ["Preboot"])
    mac.add_volume(ct, "Recovery", ["Recovery"])
    mac.add_volume(ct, "Update", ["Update"])
    data = mac.add_volume(ct, "Macintosh HD - Data", ["Data"], filevault=True)
    mac.add_volume(ct, "VM", ["VM"])
    group = mac.group(data, system, guuid=mac.new_uuid())
    mac.boot_group = mac.nvram_boot_group = group
    mac.limits[macos.uuid] = {
        "ContainerCurrentSize": container_size,
        "CurrentSize": container_size,
        "MaximumSize": container_size,
        "MinimumSizeNoGuard": 175_800_000_000,
        "MinimumSizePreferred": PREFERRED,
        "Type": "APFSPhysicalStore",
    }
    rec = mac.add_part("Apple_APFS_Recovery", *RECOVERY, gpt_name="RecoveryOSContainer")
    ct = mac.add_container(rec, used=2 * GIB)
    mac.add_volume(ct, "Recovery", ["Recovery"])
    mac.add_volume(ct, "Update", ["Update"])
    write_macos_files(mac, system, preboot, group)
    return mac


def write_macos_files(mac, system, preboot, group):
    def put(base, rel, data):
        path = os.path.join(base, rel)
        os.makedirs(os.path.dirname(path), exist_ok=True)
        with open(path, "wb") as f:
            f.write(data)

    version, build = mac.macos
    os.makedirs(os.path.join(system.dir, "Library"), exist_ok=True)
    put(system.dir, "System/Library/CoreServices/SystemVersion.plist", plist({"ProductVersion": version, "ProductBuildVersion": build}))
    put(system.dir, "System/Library/CoreServices/RestoreVersion.plist", plist({"RestoreLongVersion": "27.0.434.0.0,0"}))
    sv, sb, slong = mac.sfr
    put(system.dir, "System/Volumes/iSCPreboot/SFR/current/SystemVersion.plist", plist({"ProductVersion": sv, "ProductBuildVersion": sb}))
    put(system.dir, "System/Volumes/iSCPreboot/SFR/current/RestoreVersion.plist", plist({"RestoreLongVersion": slong}))
    put(
        preboot.dir,
        f"{group}/var/db/AdminUserRecoveryInfo.plist",
        plist(
            {
                OWNER: {"GeneratedUID": "00000000-0000-4000-8000-000000000001", "RealName": "Chris Allen"},
                "_mbsetupuser": {"GeneratedUID": "00000000-0000-4000-8000-000000000002", "RealName": "Setup User"},
            }
        ),
    )
    put(system.dir, f"Users/{OWNER}/Library/Preferences/.GlobalPreferences.plist", plist({"AppleLocale": "en_US"}))
    put(
        system.dir,
        f"Users/{OWNER}/Library/Preferences/com.apple.HIToolbox.plist",
        plist({"AppleCurrentKeyboardLayoutInputSourceID": "com.apple.keylayout.US"}),
    )
    os.makedirs(os.path.join(system.dir, "etc"), exist_ok=True)
    os.symlink("/var/db/timezone/zoneinfo/America/Chicago", os.path.join(system.dir, "etc", "localtime"))


def j514s_clean(root, **kw):
    return base_mac(root, **kw)


def j514s_free_space(root, **kw):
    """macOS already shrunk; the rest is free."""
    return base_mac(root, container_size=CONTAINER_AFTER, **kw)


def add_stub(mac, offset, size, label, with_os=True, version="14.8.3"):
    stub = mac.add_part("Apple_APFS", offset, size, gpt_name="Untitled")
    ct = mac.add_container(stub, used=1 * GIB)
    if not with_os:
        mac.add_volume(ct, label, [])
        return stub, ct, None
    data = mac.add_volume(ct, label + " - Data", ["Data"])
    system = mac.add_volume(ct, label, ["System"])
    preboot = mac.add_volume(ct, "Preboot", ["Preboot"])
    mac.add_volume(ct, "Recovery", ["Recovery"])
    group = mac.group(data, system)
    path = os.path.join(system.dir, "System/Library/CoreServices/SystemVersion-disabled.plist")
    os.makedirs(os.path.dirname(path))
    with open(path, "wb") as f:
        f.write(plist({"ProductVersion": version, "ProductBuildVersion": "23J330"}))
    os.makedirs(os.path.join(preboot.dir, group, "var/db"))
    return stub, ct, (system, data, group)


def j514s_existing_sekka(root, enrolled=None, **kw):
    """The reference J514S as it is now: Sekka installed beside macOS."""
    mac = base_mac(root, container_size=CONTAINER_AFTER, **kw)
    stub, ct, (system, data, group) = add_stub(mac, *STUB, "Sekka")
    esp = mac.add_part("EFI", *ESP, gpt_name="Untitled", volume_name="EFI - SEKKA")
    root_part = mac.add_part("Linux Filesystem", *ROOT, gpt_name="Untitled")
    installation = "8e2f6c1a-6b3d-4c1e-9a55-0d4f2f2b7c11"
    config = {
        "schema": c.RECOVERY_SCHEMA,
        "installation": installation,
        "release": "sekka-j514s-4882dc33",
        "volume_name": "Sekka",
        "stub": {
            "container": ct.uuid.lower(),
            "volume_group": group.lower(),
            "system_volume": system.uuid.lower(),
            "data_volume": data.uuid.lower(),
        },
        "esp": esp.uuid.lower(),
        "root": root_part.uuid.lower(),
        "boot_object": c.inventory_file("boot.bin", b"m1n1"),
        "firmware": [c.inventory_file("firmware/firmware.cpio", b"cpio")],
        "owner": {"short_name": OWNER, "full_name": "Chris Allen", "macos_volume_group": mac.boot_group.lower()},
        "os_firmware": "14.8.3",
    }
    data_dir = os.path.join(system.dir, c.RECOVERY_DATA_DIR)
    os.makedirs(data_dir)
    with open(os.path.join(data_dir, "recovery.json"), "wb") as f:
        f.write(canonical.dumps(config))
    with open(os.path.join(data_dir, "boot.bin"), "wb") as f:
        f.write(b"m1n1")
    os.makedirs(os.path.join(system.dir, "Sekka Recovery.app", "Contents"))
    handoff = c.handoff(installation, "sekka-j514s-4882dc33", "4882dc33652612e7475f467f6373d8b607e5407c", root_part.uuid.lower(), esp.uuid.lower(), "2026-10-18T12:00:00Z", {})
    os.makedirs(os.path.join(esp.dir, "sekka"))
    with open(os.path.join(esp.dir, c.HANDOFF_PATH), "wb") as f:
        f.write(canonical.dumps(handoff))
    mac.policies[group.upper()] = enrolled
    return mac, installation


def j514s_partial(root, **kw):
    """An installation stopped after the stub partition: an empty APFS
    container named Sekka where Sekka would go."""
    mac = base_mac(root, container_size=CONTAINER_AFTER, **kw)
    add_stub(mac, *STUB, "Sekka", with_os=False)
    return mac


def j514s_unknown(root, **kw):
    """Something no one recognizes: a stub-like system with no records, and
    a Linux partition that no stub claims."""
    mac = base_mac(root, container_size=CONTAINER_AFTER, **kw)
    stub, ct, (system, data, group) = add_stub(mac, *STUB, "Mystery")
    mac.add_part("Linux Filesystem", ROOT[0], 20 * GIB, gpt_name="Untitled")
    mac.policies[group.upper()] = None
    return mac


def m1_pro_with_asahi(root, **kw):
    """Omarchy's recorded M1 Pro (j314sap) with Asahi installed beside macOS."""
    base = os.path.join(FIXTURES, "omarchy-m1-pro-2026-09-28")

    def load(rel):
        with open(os.path.join(base, rel), "rb") as f:
            return plistlib.load(f)

    listing = load("legacy/list.plist")
    apfs = {ct["ContainerReference"]: ct for ct in load("legacy/apfs-list.plist")["Containers"]}
    groups = {ct["ContainerReference"]: ct for ct in load("legacy/apfs-volumegroups.plist")["Containers"]}
    mac = FakeMac(root, privileged=kw.get("privileged", True))
    disk = load("legacy/info/disk0.plist")
    mac.disk_size = disk["Size"]
    mac.device_class, mac.product_type, mac.chip_id, mac.board_id = "j314sap", "MacBookPro18,3", 0x6000, 0x08
    mac.product_name, mac.soc_name, mac.sys_firmware = "MacBook Pro (14-inch, 2021)", "Apple M1 Pro", "iBoot-11881.140.96"
    mac.macos = ("15.6", "24G84")
    mac.sfr = ("15.6", "24G84", "24.7.84.0.0,0")
    for entry in listing["AllDisksAndPartitions"][0]["Partitions"]:
        info = load(f"legacy/info/{entry['DeviceIdentifier']}.plist")
        p = mac.add_part(
            info["Content"],
            info["PartitionMapPartitionOffset"],
            info["Size"],
            gpt_name=info["IORegistryEntryName"],
            volume_name=info.get("VolumeName", ""),
            puuid=info["DiskUUID"],
        )
        ref = info.get("APFSContainerReference")
        if ref is None:
            for r, ct in apfs.items():
                if ct["DesignatedPhysicalStore"] == entry["DeviceIdentifier"]:
                    ref = r
        if ref is None:
            continue
        rec = apfs[ref]
        ct = mac.add_container(p, used=rec["CapacityCeiling"] - rec["CapacityFree"], cuuid=rec["APFSContainerUUID"], ref=ref)
        for vol in rec["Volumes"]:
            mac.add_volume(ct, vol["Name"], vol["Roles"], vuuid=vol["APFSVolumeUUID"], filevault=vol["FileVault"], dev=vol["DeviceIdentifier"])
        byuuid = {v.uuid: v for v in ct.volumes}
        for vg in groups[ref]["VolumeGroups"]:
            data = [byuuid.get(x["DiskUUID"]) or [v for v in ct.volumes if v.dev == x["DeviceIdentifier"]][0] for x in vg["Volumes"] if x["Role"] == "Data"][0]
            system = [[v for v in ct.volumes if v.dev == x["DeviceIdentifier"]][0] for x in vg["Volumes"] if x["Role"] == "System"][0]
            mac.group(data, system, guuid=vg["APFSVolumeGroupUUID"])
    macos_group = "3EE75828-1F54-4365-9BA7-E7217E81D869"
    stub_group = "0BF26A61-4AFF-49ED-814F-9323EE7602A4"
    mac.boot_group = mac.nvram_boot_group = macos_group
    store = mac.part_by_name("disk0s2")
    mac.limits[store.uuid] = load("after-make-space/macos-limits.plist")
    mac.policies[stub_group] = "deadbeef"
    ct_macos, system = mac.volume_by_dev("disk4s1")
    _, preboot = mac.volume_by_dev("disk4s2")
    write_macos_files(mac, system, preboot, macos_group)
    _, stub_system = mac.volume_by_dev("disk3s2")
    _, stub_preboot = mac.volume_by_dev("disk3s3")
    os.makedirs(os.path.join(stub_system.dir, "Finish Installation.app", "Contents"))
    path = os.path.join(stub_system.dir, "System/Library/CoreServices/SystemVersion.plist")
    os.makedirs(os.path.dirname(path))
    with open(path, "wb") as f:
        f.write(plist({"ProductVersion": "13.5", "ProductBuildVersion": "22G74"}))
    esp = mac.part_by_name("disk0s4")
    os.makedirs(os.path.join(esp.dir, "asahi"))
    shutil.copy(os.path.join(base, "legacy", "asahi_stub_info.json"), os.path.join(esp.dir, "asahi", "stub_info.json"))
    return mac


# Asahi with the fakes -----------------------------------------------------------------


def fake_urlcache_class(asahi, data, served):
    """Asahi's own URLCache, serving a synthetic image from memory: no
    connection is ever opened."""
    base = asahi.urlcache.URLCache

    class URLCache(base):
        def get_con(self):
            raise AssertionError("the tests never open a connection")

        def get_size(self):
            return len(data)

        def get_partial(self, off, size, bypass_cache=False):
            served.append((off, size))
            self.bytes_read += size
            return data[off : off + size]

        def close_connection(self):
            pass

    return URLCache


class FakeFW:
    def __init__(self, *args, **kwargs):
        pass

    def files(self):
        from asahi_firmware.core import FWFile

        return [("apple/fake-" + type(self).__name__ + ".bin", FWFile("apple/x", type(self).__name__.encode() * 64))]


def fake_copy_recompress(self, src, path):
    """Linux has no LZFSE or resource forks: copy the member as it is,
    reading all of it through Asahi's own package reader."""
    with self.pkg.open(src) as sfd, open(path, "wb") as dfd:
        shutil.copyfileobj(sfd, dfd)


@contextlib.contextmanager
def asahi_patches(asahi, mac, restore_zip=None, served=None):
    """What a Linux test runner needs Asahi's modules to do differently:
    libSystem's fsctl, LZFSE and resource forks don't exist here, the
    firmware collectors need real Apple firmware, absolute /System paths
    must land in the fake, and Apple's server is a byte string."""
    real_open = open

    def fake_open(path, *args, **kwargs):
        if isinstance(path, str) and path.startswith("/System/"):
            path = mac.fs(path)
        return real_open(path, *args, **kwargs)

    patches = [
        mock.patch.object(asahi.system, "open", fake_open, create=True),
        mock.patch.object(asahi.stub, "fsctl_set_bootable", lambda path, value: None),
        mock.patch.object(asahi.stub.StubInstaller, "copy_recompress", fake_copy_recompress),
    ]
    for name in ("WiFiFWCollection", "BluetoothFWCollection", "MultitouchFWCollection", "ISPFWCollection", "KernelFWCollection", "AlsFWCollection"):
        patches.append(mock.patch.object(asahi.stub, name, type(name, (FakeFW,), {})))
    if restore_zip is not None:
        patches.append(mock.patch.object(asahi.urlcache, "URLCache", fake_urlcache_class(asahi, restore_zip, served if served is not None else [])))
    with contextlib.ExitStack() as stack:
        for p in patches:
            stack.enter_context(p)
        yield


def load_asahi(mac):
    host = FakeHost(mac)
    return asahi_loader.load(asahi_root(), host), host


# A release ------------------------------------------------------------------------------

BOOT_ENTRY = "loader/entries/nixos-generation-1.conf"
REVISION = "4882dc33652612e7475f467f6373d8b607e5407c"
RELEASE = "sekka-j514s-4882dc33"


def with_directories(members):
    """Add an entry for every folder before its contents, as zip -r and
    Apple's archives do."""
    out, seen = [], set()
    for name, data, mode in members:
        parts = name.rstrip("/").split("/")
        for i in range(1, len(parts)):
            d = "/".join(parts[:i]) + "/"
            if d not in seen and not d.startswith(".."):
                seen.add(d)
                out.append((d, b"", 0o40755))
        if name.endswith("/"):
            if name in seen:
                continue
            seen.add(name)
        out.append((name, data, mode))
    return out


def stored_zip(members):
    buf = io.BytesIO()
    with zipfile.ZipFile(buf, "w", zipfile.ZIP_STORED) as zf:
        for name, data, mode in members:
            info = zipfile.ZipInfo(name, date_time=(2026, 11, 6, 0, 0, 0))
            info.external_attr = (mode & 0xFFFF) << 16
            info.create_system = 3
            zf.writestr(info, data)
    return buf.getvalue()


def make_release(directory, root_image_size=3 * MIB, recovery_members=None, tamper=None):
    """payload.zip, installer_data.json, manifest.json and recovery.zip, as
    the image producer and the release would make them."""
    rng = random.Random(7)
    esp = {
        "m1n1/boot.bin": b"m1n1 stage 2 " * 1000,
        "EFI/BOOT/BOOTAA64.EFI": b"systemd-boot" * 100,
        "EFI/systemd/systemd-bootaa64.efi": b"systemd-boot" * 100,
        "EFI/nixos/kernel.efi": bytes(rng.getrandbits(8) for _ in range(200_000)),
        "EFI/nixos/initrd.efi": bytes(rng.getrandbits(8) for _ in range(100_000)),
        "loader/loader.conf": b"timeout 3\ndefault nixos-generation-1.conf\n",
        BOOT_ENTRY: (
            b"title Sekka\nlinux /EFI/nixos/kernel.efi\ninitrd /vendorfw/firmware.cpio\ninitrd /EFI/nixos/initrd.efi\n"
            b"options init=/nix/store/x/init sekka.root=PARTUUID=@SEKKA_ROOT_PARTUUID@ "
            b"sekka.esp=PARTUUID=@SEKKA_ESP_PARTUUID@ quiet\n"
        ),
    }
    root_image = bytes(rng.getrandbits(8) for _ in range(4096)) * (root_image_size // 4096)
    members = [("esp/", b"", 0o40755)]
    dirs = set()
    for path in sorted(esp):
        parts = path.split("/")
        for i in range(1, len(parts)):
            d = "esp/" + "/".join(parts[:i]) + "/"
            if d not in dirs:
                dirs.add(d)
                members.append((d, b"", 0o40755))
        members.append(("esp/" + path, esp[path], 0o100644))
    members.append(("root.img", root_image, 0o100644))
    if tamper == "payload-extra":
        members.append(("../escape", b"x", 0o100644))
    payload = stored_zip(members)
    installer_data = {
        "os_list": [
            {
                "name": "Sekka (test)",
                "default_os_name": "Sekka",
                "boot_object": "m1n1.bin",
                "next_object": "m1n1/boot.bin",
                "package": "sekka-j514s-test.zip",
                "supported_fw": ["14.8.3"],
                "partitions": [
                    {"name": "EFI", "type": "EFI", "size": f"{GIB}B", "format": "fat", "volume_id": "0x5ecca5e1", "copy_firmware": True, "copy_installer_data": True, "source": "esp"},
                    {"name": "Root", "type": "Linux", "size": f"{len(root_image)}B", "expand": True, "image": "root.img"},
                ],
            }
        ]
    }
    manifest = {
        "schema": "sekka.mac.manifest/1",
        "release": RELEASE,
        "source_revision": REVISION,
        "root_image": {"path": "root.img", "size": len(root_image), "digest": canonical.digest_bytes(root_image)},
        "esp": {
            "boot_entry": BOOT_ENTRY,
            "files": [{"path": p, "size": len(d), "digest": canonical.digest_bytes(d)} for p, d in sorted(esp.items())],
        },
    }
    if recovery_members is None:
        recovery_members = [
            ("Sekka Recovery.app/", b"", 0o40755),
            ("Sekka Recovery.app/Contents/", b"", 0o40755),
            ("Sekka Recovery.app/Contents/Info.plist", plist({"CFBundleExecutable": "Sekka Recovery"}), 0o100644),
            ("Sekka Recovery.app/Contents/MacOS/", b"", 0o40755),
            ("Sekka Recovery.app/Contents/MacOS/Sekka Recovery", b"\xcf\xfa\xed\xfe" + b"\0" * 4000, 0o100755),
            ("Sekka Recovery.app/Contents/_CodeSignature/CodeResources", b"<plist/>", 0o100644),
        ]
    recovery = stored_zip(recovery_members)
    files = {
        "payload.zip": payload,
        "installer_data.json": json.dumps(installer_data, indent=2).encode(),
        "manifest.json": json.dumps(manifest, indent=2).encode(),
        "recovery.zip": recovery,
    }
    os.makedirs(directory, exist_ok=True)
    for name, data in files.items():
        with open(os.path.join(directory, name), "wb") as f:
            f.write(data)
    digests = {name: canonical.digest_bytes(data) for name, data in files.items()}
    return {
        "dir": directory,
        "digests": digests,
        "root_image": root_image,
        "root_digest": manifest["root_image"]["digest"],
        "esp": esp,
        "manifest": manifest,
    }


def derive_plan(inspection, release, sekka, place="shrink", profile=None):
    """A port of plan.rs derive_plan for the tests (the helper derives the
    real one in Rust; the vectors test checks they agree on bytes)."""
    profile = profile or c.load_profile(profile_bytes())[0]
    align = c.ALIGN
    sekka = sekka // align * align
    root = sekka - c.STUB_BYTES - c.ESP_BYTES
    cand = [k for k in inspection["candidates"] if k["kind"] == ("shrink-container" if place == "shrink" else "free-space")][0]
    if sekka > (c.candidate_capacity(cand) // align * align) - align:
        raise ValueError("TooLarge: plan.rs derive_plan refuses this size here")
    if place == "shrink":
        cand = [k for k in inspection["candidates"] if k["kind"] == "shrink-container"][0]
        part = [p for p in inspection["disk"]["partitions"] if p["uuid"] == cand["partition"]][0]
        end = part["offset"] + cand["current_size"] + cand["adjacent_free"]
        offset = (end - sekka) // align * align
        to = offset - part["offset"]
        actions = [{"kind": "shrink-container", "container": cand["container"], "from": cand["current_size"], "to": to}]
        after = to
    else:
        cand = [k for k in inspection["candidates"] if k["kind"] == "free-space"][0]
        offset = -(-cand["extent"]["offset"] // align) * align
        actions = []
        after = None
    actions += [
        {"kind": "create-stub", "size": c.STUB_BYTES},
        {"kind": "create-esp", "size": c.ESP_BYTES},
        {"kind": "create-root", "size": root},
    ]
    m = inspection["machine"]
    acknowledgements = ["known-macos-issue", "development-release"]
    plan = {
        "schema": c.PLAN_SCHEMA,
        "release": {
            "channel": "development",
            "catalog_serial": 1,
            "release": RELEASE,
            "profile": profile["id"],
            "profile_digest": canonical.digest(profile),
            "source_revision": REVISION,
            "payload": release["digests"]["payload.zip"],
            "installer_data": release["digests"]["installer_data.json"],
            "manifest": release["digests"]["manifest.json"],
            "engine": canonical.digest_bytes(b"engine"),
            "recovery_app": release["digests"]["recovery.zip"],
            "root_image": release["root_digest"],
        },
        "machine": {k: m[k] for k in ("product_type", "device_class", "chip_id", "system_firmware", "binding")},
        "disk": {
            "disk": inspection["disk"]["uuid"],
            "layout_digest": inspection["layout_digest"],
            "macos_volume_group": inspection["macos"]["volume_group"],
        },
        "allocation": {"sekka": sekka, "stub": c.STUB_BYTES, "esp": c.ESP_BYTES, "root": root, "macos_container_after": after},
        "actions": actions,
        "target": {"offset": offset, "size": sekka},
        "volume_name": "Sekka",
        "human_steps": ["shut-down", "hold-power-button", "choose-volume", "authorize-in-recovery", "create-account"],
        "acknowledgements": acknowledgements,
    }
    c.check_plan(plan)
    return plan


def make_restore_zip():
    """A small stand-in for Apple's macOS 14.8.3 update for the J514S, laid
    out as Asahi's OTA path mapping expects (AssetData/boot, AssetData/
    Restore), with members for another device that must not be fetched."""
    variant = "macOS Customer Software Update"
    manifest = {
        "BuildIdentities": [
            {
                "ApBoardID": "0x44",
                "ApChipID": "0x6030",
                "Info": {"DeviceClass": "j514sap", "RestoreBehavior": "Update", "Variant": variant, "BuildNumber": "23J330"},
                "Manifest": {
                    "iBoot": {"Info": {"Path": "Firmware/all_flash/iBoot.j514s.RELEASE.im4p"}},
                    "KernelCache": {"Info": {"Path": "kernelcache.release.mac15s"}},
                    "Multitouch": {"Info": {"Path": "Firmware/MT/j514s.im4p", "IsFUDFirmware": True}},
                    "ANE": {"Info": {"Path": "Firmware/ane/j514s.im4p", "IsFUDFirmware": True, "IsLoadedByiBoot": True}},
                    "BaseSystem": {"Info": {"Path": "x.dmg"}},
                    "OS": {"Info": {"Path": "os.dmg"}},
                    "RestoreRamDisk": {"Info": {"Path": "ramdisk.dmg"}},
                    "Cryptex1,SystemOS": {"Info": {"Path": "cryptex.dmg"}},
                },
            },
            {
                "ApBoardID": "0x08",
                "ApChipID": "0x6000",
                "Info": {"DeviceClass": "j314sap", "RestoreBehavior": "Update", "Variant": variant, "BuildNumber": "23J330"},
                "Manifest": {"iBoot": {"Info": {"Path": "Firmware/all_flash/iBoot.j314s.RELEASE.im4p"}}},
            },
        ]
    }
    rng = random.Random(3)
    boot = {
        "BuildManifest.plist": plist(manifest),
        "usr/standalone/bootcaches.plist": plist({"bless2": {"RestoreBundlePath": "./Restore"}}),
        "SystemVersion.plist": plist({"ProductVersion": "14.8.3", "ProductBuildVersion": "23J330", "ProductUserVisibleVersion": "14.8.3"}),
        "PlatformSupport.plist": plist({"SupportedBoardIds": []}),
        "RestoreVersion.plist": plist({"RestoreLongVersion": "23.10.330.0.0,0"}),
        f"Firmware/Manifests/restore/{variant}/apticket.der": b"ticket",
        "Firmware/all_flash/iBoot.j514s.RELEASE.im4p": b"iboot-j514s" * 100,
        "Firmware/all_flash/iBoot.j314s.RELEASE.im4p": bytes(rng.getrandbits(8) for _ in range(3 * MIB)),
        "kernelcache.release.mac15s": b"kernel" * 1000,
        "Firmware/MT/j514s.im4p": b"multitouch" * 100,
        "Firmware/ane/j514s.im4p": b"ane" * 100,
    }
    restore = {
        "BootabilityBundle/Restore/Bootability/bootability.dmg": b"bootability" * 100,
        "BootabilityBundle/Restore/Firmware/Bootability.dmg.trustcache": b"trustcache",
    }
    members = [("AssetData/boot/" + k, v, 0o100644) for k, v in boot.items()]
    members += [("AssetData/Restore/" + k, v, 0o100644) for k, v in restore.items()]
    members.append(("AssetData/payloadv2/basesystem_patches/arm64eBaseSystem.dmg", b"BXDIFF50" + b"\0" * 5000, 0o100644))
    members.append(("AssetData/payloadv2/payload.000", bytes(rng.getrandbits(8) for _ in range(2 * MIB)), 0o100644))
    return stored_zip(with_directories(members))


# Running the engine against the fake ------------------------------------------------------


class Channel:
    """The transcript's two ends as the helper sees them: it records every
    line, and answers each intent with its acknowledgement, noting the
    fake Mac's state at that moment."""

    def __init__(self, mac, ack=True, stop_at=None):
        self.mac = mac
        self.lines = []
        self.ack = ack
        self.stop_at = stop_at
        self.pending = b""
        self.events = []
        self.buffer = b""

    # the engine's output
    def write(self, data):
        self.buffer += data
        while b"\n" in self.buffer:
            line, self.buffer = self.buffer.split(b"\n", 1)
            self.lines.append(line)
            message = json.loads(line)["message"]
            if message["type"] in ("checkpoint", "failure"):
                self.events.append((message["type"], message["payload"].get("stage"), self.mac.state_digest()))
        return len(data)

    def flush(self):
        pass

    # the engine's input
    def read(self, n):
        if not self.pending:
            message = json.loads(self.lines[-1])
            assert message["message"]["type"] == "intent", "an acknowledgement was read without an intent"
            stage = message["message"]["payload"]["stage"]
            self.events.append(("ack", stage, self.mac.state_digest()))
            if not self.ack or stage == self.stop_at:
                return b""
            self.pending = canonical.dumps({"ack": message["seq"]}) + b"\n"
        out, self.pending = self.pending[:n], self.pending[n:]
        return out

    def messages(self):
        return [json.loads(line) for line in self.lines]


def run_engine(mode, mac, asahi, host, workdir, plan=None, release=None, password=PASSWORD, owner=OWNER, installation="1f7d9c62-4a1b-4c8e-9d3f-2b6a8e0c5d47", channel=None, privileged=True, restore=None):
    from sekka_engine.engine import Engine

    channel = channel or Channel(mac)
    engine = Engine(
        host=host,
        asahi=asahi,
        profile_bytes=profile_bytes(),
        out=channel,
        acks=channel,
        adapter=ADAPTER,
        workdir=workdir,
        password=bytearray(password) if password is not None else None,
        privileged=privileged,
    )
    served = []
    quiet = io.StringIO()  # Asahi's progress printing; a real run sends it to stderr
    with asahi_patches(asahi, mac, restore_zip=restore if restore is not None else make_restore_zip(), served=served), contextlib.redirect_stdout(quiet), warnings.catch_warnings():
        # Asahi's modules leave plist files for the garbage collector.
        warnings.simplefilter("ignore", ResourceWarning)
        if mode == "inspect":
            outcome = engine.run_inspect()
        elif mode == "startup-disk":
            plan_bytes = canonical.dumps(plan) if isinstance(plan, dict) else plan
            outcome = engine.run_startup_disk(plan_bytes, installation, owner)
        else:
            plan_bytes = canonical.dumps(plan) if isinstance(plan, dict) else plan
            outcome = engine.run_execute(plan_bytes, release["dir"], installation, owner)
    channel.served = served
    channel.engine = engine
    return outcome, channel


def inspect(mac, asahi, host, workdir, privileged=True):
    outcome, channel = run_engine("inspect", mac, asahi, host, workdir, privileged=privileged)
    docs = [m["message"]["payload"] for m in channel.messages() if m["message"]["type"] == "inspection"]
    return outcome, (docs[0] if docs else None), channel


class TempDir:
    def __init__(self):
        self.path = tempfile.mkdtemp(prefix="sekka-engine-test-")

    def sub(self, *parts):
        path = os.path.join(self.path, *parts)
        os.makedirs(path, exist_ok=True)
        return path

    def cleanup(self):
        shutil.rmtree(self.path, ignore_errors=True)
