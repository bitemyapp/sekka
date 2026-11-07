# SPDX-License-Identifier: GPL-3.0-or-later
"""Build the contract's Inspection from Asahi's own model of the Mac.

Everything here reads; nothing changes a disk. Whether it runs as root or
not, volumes are mounted read-only and unmounted again, and nothing already
mounted is touched. Run unprivileged (the app inspects before it asks for a
password), what only root can read (boot policies, the admin-user records) is
reported as unavailable rather than guessed.
"""

import hashlib
import logging
import os
import re
import struct
import uuid as uuidlib

from . import canonical
from . import contract as c

log = logging.getLogger("sekka_engine.inspect")

PARTITION_KINDS = {
    "Apple_APFS_ISC": "apple-isc",
    "Apple_APFS": "apfs",
    "Apple_APFS_Recovery": "apple-recovery",
    "EFI": "efi",
    "Linux Filesystem": "linux",
    "Linux": "linux",
    "Linux Swap": "linux-swap",
}

RECOVERY_APP = "Sekka Recovery.app"
ASAHI_STEP2_APP = "Finish Installation.app"

# Sekka's developer image (nix/apple-silicon/developer/developer.nix), which
# install-sekka.sh installs with the unmodified Asahi installer, leaves no
# handoff and no Recovery configuration. It mounts its EFI system partition
# by the fixed FAT serial its installer data gives newfs_msdos, 5ECC-A5E1
# (nix/apple-silicon/identities.json, "developer"; tests.nix holds this to
# it), and its root by the label sekka-root. macOS can't read ext4, so
# diskutil reports no label or UUID for the root: the serial is the sign
# macOS can see.
DEVELOPER_ESP_SERIAL = 0x5ecca5e1
DEVELOPER_ESP_NAME = "%04X-%04X" % divmod(DEVELOPER_ESP_SERIAL, 0x10000)
# What every image's ESP holds: Asahi's second stage, and the record Sekka's
# boot-bin keeps of it (nix/apple-silicon/boot-bin/boot_bin.py), which
# nothing but Sekka writes. A developer image older than boot-bin has only
# the first.
BOOT_BIN = "m1n1/boot.bin"
BOOT_BIN_RECORD = "sekka/boot-bin.json"
REMOVAL_DOC = "docs/apple-silicon/removal-and-boards.md"
# msdosfs's kFSUUIDNamespaceSHA1 (msdosfs_vfsops.c).
FAT_UUID_NAMESPACE = uuidlib.UUID("b3e20f39-f292-11d6-97a4-00306543ecac").bytes


class InspectionError(RuntimeError):
    """The Mac couldn't be described with confidence."""


def fat_volume_uuid(serial, sectors):
    """The VolumeUUID diskutil reports for a FAT volume, as macOS's msdosfs
    derives it (msdosfs_generate_volume_uuid): a version 3 UUID, the MD5 of
    kFSUUIDNamespaceSHA1, the boot sector's volume ID as stored and its total
    sector count, both little-endian. Given the volume's size, the UUID says
    which serial its boot sector has."""
    digest = bytearray(
        hashlib.md5(FAT_UUID_NAMESPACE + struct.pack("<II", serial, sectors), usedforsecurity=False).digest()
    )
    digest[6] = (digest[6] & 0x0F) | 0x30
    digest[8] = (digest[8] & 0x3F) | 0x80
    return str(uuidlib.UUID(bytes=bytes(digest)))


def has_developer_serial(part):
    """Whether diskutil's description of an EFI partition gives it the
    developer image's FAT serial. newfs_msdos counts the whole partition in
    the disk's blocks, as on the recorded M1 Pro's ESP. A partition with no
    FAT UUID, or one whose size can't be counted that way, hasn't."""
    info = part.info or {}
    volume, size, block = info.get("VolumeUUID"), info.get("Size"), info.get("DeviceBlockSize")
    if not isinstance(size, int) or not isinstance(block, int) or block <= 0 or size % block:
        return False
    try:
        return c.uuid_any(volume) == fat_volume_uuid(DEVELOPER_ESP_SERIAL, size // block)
    except c.Invalid:
        return False


def listed(names):
    names = list(names)
    return names[0] if len(names) == 1 else ", ".join(names[:-1]) + " and " + names[-1]


def size_text(size):
    """A size as `diskutil list` prints it: decimal units, one place."""
    for power, unit in ((4, "TB"), (3, "GB"), (2, "MB"), (1, "KB")):
        if size >= 1000**power:
            return f"{size / 1000**power:.1f} {unit}"
    return f"{size} B"


def removal(stub, esp, roots, name=None):
    """How the owner removes a developer installation (or what is left of
    one) before installing, naming its partitions by what `diskutil list`
    shows of them: diskN numbers aren't stable once one is deleted
    (removal-and-boards.md, after Asahi's partitioning cheatsheet). `stub`
    is its APFS container, named `name`."""
    parts = []
    if stub is not None:
        named = name or stub.label or stub.name
        parts.append(f'its APFS container "{named}" ({size_text(stub.size)}, now {stub.name})')
    if esp is not None:
        volume = (esp.info or {}).get("VolumeName")
        named = f' "{volume}"' if isinstance(volume, str) and volume else ""
        parts.append(f"its EFI partition{named} ({size_text(esp.size)}, now {esp.name})")
    for q in roots:
        parts.append(f"its Linux partition ({size_text(q.size)}, now {q.name})")
    startup = ", make macOS the startup disk" if stub is not None else ""
    return (
        f"To install Sekka here, copy off what you want to keep{startup}, then delete {listed(parts)} in macOS with "
        f"diskutil, as {REMOVAL_DOC} describes. Partition numbers can change once one is deleted, so check each one "
        "with diskutil list before deleting it."
    )


def walk(node):
    yield node
    for child in node.get("IORegistryEntryChildren", None) or []:
        yield from walk(child)


def platform_uuid(sysinfo):
    """IOPlatformUUID, from the IOService plane Asahi's SystemInfo read."""
    for node in walk(sysinfo.ioservice):
        if node.get("IOObjectClass") == "IOPlatformExpertDevice" and "IOPlatformUUID" in node:
            return node["IOPlatformUUID"]
    raise InspectionError("this Mac's hardware UUID isn't in the I/O Registry")


def gpt_disk_uuid(sysinfo, bsd_name):
    """The GPT disk GUID IOGUIDPartitionScheme publishes as `UUID` under the
    whole disk's IOMedia (IOStorageFamily's IOGUIDPartitionScheme.cpp)."""
    for node in walk(sysinfo.ioservice):
        if node.get("BSD Name") == bsd_name and node.get("Whole") is True:
            for child in node.get("IORegistryEntryChildren", None) or []:
                if child.get("IOObjectClass") == "IOGUIDPartitionScheme" and "UUID" in child:
                    return c.uuid_any(child["UUID"])
    raise InspectionError(f"{bsd_name}'s GPT identity isn't in the I/O Registry")


def run_text(host, argv):
    result = host.run(argv, timeout=120)
    return result.returncode, (result.stdout or b"").decode("utf-8", "replace")


def read_power(host):
    """pmset -g batt: 'AC Power' or 'Battery Power', and the internal
    battery's charge."""
    rc, out = run_text(host, ["pmset", "-g", "batt"])
    if rc != 0 or ("'AC Power'" not in out and "'Battery Power'" not in out and "'UPS Power'" not in out):
        return None
    has_battery = "InternalBattery" in out
    percent = None
    match = re.search(r"InternalBattery-\d+[^\n]*?\t(\d{1,3})%", out)
    if match and int(match.group(1)) <= 100:
        percent = int(match.group(1))
    on_ac = "'AC Power'" in out or not has_battery
    return {"has_battery": has_battery, "on_ac": on_ac, "battery_percent": percent}


def read_filevault(host, data_volume):
    rc, out = run_text(host, ["fdesetup", "status"])
    if rc == 0:
        if "Encryption in progress" in out or "Decryption in progress" in out:
            return "converting"
        if "FileVault is On" in out:
            return "on"
        if "FileVault is Off" in out:
            return "off"
    # Asahi's own model has the volume's state from `diskutil apfs list`.
    if data_volume is not None:
        if data_volume.get("CryptoMigrationOn") is True:
            return "converting"
        if data_volume.get("FileVault") is True:
            return "on"
        if data_volume.get("FileVault") is False:
            return "off"
    return "unknown"


def parse_version(value):
    try:
        c.version(value)
        return value
    except (c.Invalid, TypeError):
        return None


class Inspector:
    """One inspection: Asahi's View plus what Sekka needs on top."""

    def __init__(self, asahi, host, profile, view, privileged):
        self.a = asahi
        self.host = host
        self.profile = profile
        self.view = view
        self.privileged = privileged
        self.issues = []
        self.unavailable = []
        self.notes = []

    def issue(self, code, blocking, detail):
        if code not in c.ISSUE_CODES:
            raise ValueError(code)
        self.issues.append({"code": code, "blocking": blocking, "detail": c.text(c.plain_text(detail, keep="start"))})

    # The parts ---------------------------------------------------------------

    def machine(self):
        s = self.view.sysinfo
        binding = c.machine_binding(platform_uuid(s))
        return {
            "product_type": c.text(s.product_type),
            "device_class": c.text(s.device_class),
            "chip_id": c.u32(s.chip_id),
            "board_id": c.u32(s.board_id),
            "product_name": c.text(s.product_name),
            "soc_name": c.text(s.soc_name),
            "system_firmware": c.text(s.sys_firmware),
            "binding": binding,
        }

    def disk(self):
        v = self.view
        whole = v.dutil.disks[v.sys_disk]
        size = v.dutil.disk_parts[v.sys_disk]["Size"]
        partitions, containers = [], []
        for p in v.parts:
            if p.free:
                continue
            info = p.info or {}
            kind = PARTITION_KINDS.get(p.type, "other")
            if kind == "efi" and info.get("VolumeName"):
                name = info["VolumeName"]
            else:
                name = info.get("IORegistryEntryName") or p.label or p.type or ""
            partitions.append(
                {
                    "uuid": c.uuid_any(p.uuid),
                    "kind": kind,
                    "name": c.plain_text(name, limit=255, keep="start"),
                    "offset": c.u64(p.offset),
                    "size": c.u64(p.size),
                }
            )
            if kind == "apfs" and p.container is not None:
                containers.append(self.container(p))
        doc = {
            "device": c.text(v.sys_disk),
            "uuid": gpt_disk_uuid(v.sysinfo, v.sys_disk),
            "size": c.u64(size),
            "block_size": c.u32(whole.get("DeviceBlockSize", 0)),
            "partitions": partitions,
            "containers": containers,
        }
        c.check_disk(doc)
        return doc

    def container(self, p):
        ct = p.container
        volumes = {vol["DeviceIdentifier"]: vol for vol in ct.get("Volumes", [])}
        groups = []
        for vg in ct.get("VolumeGroups", []) or []:
            gid = c.uuid_any(vg["APFSVolumeGroupUUID"])
            system = [x for x in vg.get("Volumes", []) if x.get("Role") == "System"]
            data = [x for x in vg.get("Volumes", []) if x.get("Role") == "Data"]
            name = ""
            if system and system[0]["DeviceIdentifier"] in volumes:
                name = volumes[system[0]["DeviceIdentifier"]].get("Name", "")
            elif data and data[0]["DeviceIdentifier"] in volumes:
                name = volumes[data[0]["DeviceIdentifier"]].get("Name", "").removesuffix(" - Data")
            groups.append(
                {
                    "uuid": gid,
                    "name": c.plain_text(name, limit=255, keep="start"),
                    "system_volume": c.uuid_any(system[0]["DiskUUID"]) if len(system) == 1 else None,
                    "data_volume": c.uuid_any(data[0]["DiskUUID"]) if len(data) == 1 else None,
                    "os": self.group_os(p, gid),
                }
            )
        resize = self.resize_limits(p) if self.holds_macos(p) else None
        return {
            "uuid": c.uuid_any(ct["APFSContainerUUID"]),
            "partition": c.uuid_any(p.uuid),
            "size": c.u64(ct["CapacityCeiling"]),
            "free": c.u64(ct["CapacityFree"]),
            "volume_groups": groups,
            "resize": resize,
            "snapshots": self.snapshots(p),
        }

    def group_os(self, p, gid):
        for o in p.os or []:
            if (o.vgid or "").lower() != gid:
                continue
            if o.stub:
                return {"kind": "stub", "for_system": c.plain_text(o.label or "", limit=255, keep="start")}
            version = parse_version(o.version)
            if version:
                return {"kind": "macos", "version": version}
        return {"kind": "unknown"}

    def holds_macos(self, p):
        return any(o.version and not o.stub for o in (p.os or []))

    def min_free(self, p):
        """Asahi's get_min_free_space: macOS keeps 38 GB (MIN_FREE_OS) for its
        updates, except on 128 GB education Macs."""
        main = self.a.main
        edu = self.view.dutil.get_disk_size(self.view.sys_disk) < 150 * 1024 * 1024 * 1024
        if p.os and any(o.version for o in p.os) and not edu:
            return main.MIN_FREE_OS
        return main.MIN_FREE

    def resize_limits(self, p):
        """Asahi's action_resize arithmetic: the smallest the container may
        become is the larger of its used space plus macOS's reserve (aligned)
        and diskutil's preferred minimum."""
        main, util = self.a.main, self.a.util
        ct = p.container
        try:
            limits = self.view.dutil.get_resize_limits(p.name)
            preferred = int(limits["MinimumSizePreferred"])
        except Exception as error:
            log.warning("resize limits for %s: %s", p.name, error)
            self.notes.append(("resize-limits", p.uuid))
            return None
        total, free = ct["CapacityCeiling"], ct["CapacityFree"]
        min_size_raw = util.align_up(total - free + self.min_free(p), main.PART_ALIGN)
        minimum = max(min_size_raw, preferred)
        return {"minimum": c.u64(minimum), "recommended": c.u64(max(minimum, preferred))}

    def snapshots(self, p):
        """Local snapshots that can stop a shrink: every snapshot on a
        container's volumes except a System volume's own sealed one, which
        diskutil marks as not limiting."""
        count = 0
        for vol in p.container.get("Volumes", []):
            roles = vol.get("Roles") or []
            try:
                listing = self.view.dutil.get("apfs", "listSnapshots", vol["DeviceIdentifier"], "-plist")
            except Exception:
                if "Data" in roles and self.holds_macos(p):
                    self.notes.append(("snapshots", vol["DeviceIdentifier"]))
                continue
            for snap in listing.get("Snapshots", []) or []:
                if "System" in roles and snap.get("LimitingContainerShrink") is not True:
                    continue
                count += 1
        return c.u32(count)

    # Recognizing installations ----------------------------------------------

    def read_mounted(self, mountpoint, *path):
        if not mountpoint:
            return None
        full = os.path.join(mountpoint, *path)
        try:
            with open(full, "rb") as f:
                return f.read(c.LIMIT_RECOVERY + 1)
        except OSError:
            return None

    def esp_mount(self, part):
        try:
            return self.view.dutil.mount(part.name)
        except Exception as error:
            log.info("couldn't mount %s to look at it: %s", part.name, error)
            return None

    def boot_files(self, mountpoint):
        """Which of every image's boot files an ESP mounted at `mountpoint`
        holds, or None if it couldn't be mounted."""
        if not mountpoint:
            return None
        return {rel: os.path.isfile(os.path.join(mountpoint, rel)) for rel in (BOOT_BIN, BOOT_BIN_RECORD)}

    def asahi_state(self, o, bp_known, enrolled):
        """A system Asahi's installer made, by its boot policy and its stub's
        Recovery step."""
        if not bp_known:
            return "unknown"
        if enrolled:
            return "complete"
        if os.path.isdir(os.path.join(o.system or "", ASAHI_STEP2_APP)):
            return "waiting-for-recovery"
        return "partial"

    def installations(self, cur_os):
        v = self.view
        found = []
        attached = set()
        for i, p in enumerate(v.parts):
            if p.free or p.type != "Apple_APFS" or p.container is None:
                continue
            oses = p.os or []
            if cur_os is not None and any(o is cur_os for o in oses):
                continue
            if any(o.version and not o.stub for o in oses):
                continue  # another macOS: not an installation of a second system
            stubs = [o for o in oses if o.stub]
            if not stubs:
                container = self.empty_container(p, i)
                found.append(container)
                if container["esp"] is not None:
                    attached.update(q.uuid for q in self.following(i))
                continue
            for o in stubs:
                for q in o.attached_partitions or []:
                    attached.add(q.uuid)
                found.append(self.stub_installation(p, o))
        for i, p in enumerate(v.parts):
            if p.free or p.uuid in attached:
                continue
            if p.type in ("EFI", "Linux Filesystem", "Linux"):
                orphan, claimed = self.orphan(p, i, attached)
                found.append(orphan)
                attached.update(q.uuid for q in claimed)
        return [x for x in found if x is not None]

    def following(self, index):
        """The partitions OSEnum.collect would attach to a stub at `index`:
        an ESP directly after it, then Linux partitions after that."""
        parts = self.view.parts
        if index + 1 >= len(parts) or parts[index + 1].free or parts[index + 1].type != "EFI":
            return []
        out = [parts[index + 1]]
        for q in parts[index + 2 : min(len(parts), index + 5)]:
            if not q.free and q.type == "Linux Filesystem":
                out.append(q)
        return out

    def linux_after(self, index, attached=()):
        """The Linux partitions directly after the ESP at `index`, as many
        as OSEnum.collect would attach with it, stopping at anything else:
        what install-sekka.sh's Asahi installer put after a developer ESP."""
        out = []
        for q in self.view.parts[index + 1 : index + 4]:
            if q.free or q.type != "Linux Filesystem" or q.uuid in attached:
                break
            out.append(q)
        return out

    def empty_container(self, p, index):
        """An APFS container that holds no system: an interrupted stub, or
        something else. Nothing is assumed about it: it is a developer
        installation's stub only when it could be a stub (no more volumes
        than one has, and named Sekka or no larger than one) and the ESP
        after it has the developer image's serial. A container of the
        owner's own, made where a developer stub was, isn't, and the
        developer ESP after it is reported as left behind."""
        label = p.label or ""
        vols = p.container.get("Volumes", [])
        nxt = self.view.parts[index + 1] if index + 1 < len(self.view.parts) else None
        esp = c.uuid_any(nxt.uuid) if nxt is not None and not nxt.free and nxt.type == "EFI" else None
        stub_like = len(vols) <= 4 and (label == c.VOLUME_NAME or p.size <= self.a.main.STUB_SIZE)
        developer_esp = esp is not None and has_developer_serial(nxt)
        developer = developer_esp and stub_like
        roots = []
        if developer:
            roots = [q for q in self.following(index) if q.type != "EFI"]
            self.issue(
                "partial-installation",
                True,
                f"The APFS container {p.name} ({label or 'unnamed'}) holds no system, and the EFI partition "
                f"after it, {nxt.name}, has the developer image's serial, {DEVELOPER_ESP_NAME}: a developer "
                "installation of Sekka may have stopped part way. The installer won't change it or install "
                f"beside it. {removal(p, nxt, roots, name=label or None)}",
            )
        elif label == c.VOLUME_NAME and len(vols) <= 4:
            self.issue(
                "partial-installation",
                True,
                f"The APFS container {p.name} is named {label} but holds no system: an earlier "
                "installation may have stopped part way. The installer won't change it; remove it "
                "in Disk Utility, or ask for help, before installing again.",
            )
        else:
            self.issue(
                "unrecognized-layout",
                False,
                f"The APFS container {p.name} ({label or 'unnamed'}) holds no system the installer "
                "recognizes. It will be left as it is.",
            )
        if developer_esp and not developer:
            # Not this container's to claim: the ESP is reported on its own.
            esp = None
        return {
            "kind": "sekka" if developer else "unknown",
            "name": c.plain_text(label or p.name, limit=255, keep="start"),
            "state": "partial",
            "stub_group": None,
            "esp": esp,
            "root": c.uuid_any(roots[0].uuid) if roots else None,
            "installation_id": None,
            "developer": developer,
        }

    def stub_installation(self, p, o):
        esp_part = None
        root_part = None
        for q in o.attached_partitions or []:
            if q.type == "EFI" and esp_part is None:
                esp_part = q
            elif q.type in ("Linux Filesystem", "Linux") and root_part is None:
                root_part = q
        config = handoff = None
        raw = self.read_mounted(o.system, c.RECOVERY_DATA_DIR, c.RECOVERY_CONFIG_FILE)
        if raw is not None:
            try:
                config = c.parse_recovery_config(raw)
            except (c.Invalid, canonical.CanonicalError, KeyError, TypeError) as error:
                log.info("%s: unreadable Sekka Recovery configuration: %s", o.label, error)
                config = False
        esp_mount = self.esp_mount(esp_part) if esp_part is not None else None
        raw = self.read_mounted(esp_mount, c.HANDOFF_PATH)
        if raw is not None:
            try:
                handoff = c.parse_handoff(raw)
            except (c.Invalid, canonical.CanonicalError, KeyError, TypeError) as error:
                log.info("%s: unreadable Sekka handoff: %s", o.label, error)
                handoff = False
        name = c.plain_text(o.label or p.label or p.name, limit=255, keep="start")
        stub_group = c.uuid_any(o.vgid)
        esp = c.uuid_any(esp_part.uuid) if esp_part is not None else None
        root = c.uuid_any(root_part.uuid) if root_part is not None else None
        bp_known = o.bp is not None
        enrolled = bool(o.bp and o.bp.get("coih"))
        if config is not None or handoff is not None:
            ids = set()
            consistent = config is not False and handoff is not False
            if config:
                ids.add(config["installation"])
                consistent &= config["esp"] == esp and config["root"] == root
            if handoff:
                ids.add(handoff["installation"])
                consistent &= handoff["esp"] == esp and handoff["root"] == root
            consistent &= len(ids) == 1
            present = (
                bool(config)
                and bool(handoff)
                and esp is not None
                and root is not None
                and os.path.isdir(os.path.join(o.system, RECOVERY_APP))
                and os.path.isfile(os.path.join(o.system, c.RECOVERY_DATA_DIR, "boot.bin"))
            )
            if not consistent:
                state = "unknown"
            elif not present:
                state = "partial"
            elif not bp_known:
                state = "unknown"
            else:
                state = "complete" if enrolled else "waiting-for-recovery"
            if state == "partial":
                self.issue(
                    "partial-installation",
                    True,
                    f"Sekka's installation on {name} is incomplete: some of its parts are missing. "
                    "The installer won't repair or remove it yet; nothing will be changed.",
                )
            else:
                self.issue(
                    "existing-sekka",
                    True,
                    f"Sekka is already installed on this Mac ({name}). A second installation isn't "
                    "offered yet, and the installer won't change the existing one.",
                )
            return {
                "kind": "sekka",
                "name": name,
                "state": state,
                "stub_group": stub_group,
                "esp": esp,
                "root": root,
                "installation_id": next(iter(ids)) if len(ids) == 1 else None,
                "developer": False,
            }
        # No installer's records: Sekka's developer image, by its ESP's
        # serial, or Sekka's boot record on an ESP nothing else identifies.
        files = self.boot_files(esp_mount)
        developer = esp_part is not None and has_developer_serial(esp_part)
        if developer or (files and files[BOOT_BIN_RECORD]):
            roots = [q for q in o.attached_partitions or [] if q.type in ("Linux Filesystem", "Linux")]
            remove = removal(p, esp_part, roots, name=name)
            if not developer:
                # Sekka's, but the developer image's or the installer's whose
                # handoff is gone? Neither is guessed.
                state = "unknown"
                self.issue(
                    "existing-sekka",
                    True,
                    f"Sekka's boot record ({BOOT_BIN_RECORD}) is on {esp_part.name}, the EFI partition of {name}, "
                    "with neither the installer's records nor the developer image's serial, so the installer "
                    f"can't tell which Sekka installation it is. It won't install beside it or change it. {remove}",
                )
            else:
                if files is None:
                    state = "unknown"
                elif root_part is None or not any(files.values()):
                    state = "partial"
                else:
                    state = self.asahi_state(o, bp_known, enrolled)
                serial = f"its EFI partition {esp_part.name} has the developer image's serial, {DEVELOPER_ESP_NAME}"
                if state == "partial":
                    if root_part is None:
                        missing = "no Linux partition follows it"
                    elif not any(files.values()):
                        missing = f"{esp_part.name} holds none of its boot files"
                    else:
                        missing = "its startup volume was never set up to start it"
                    self.issue(
                        "partial-installation",
                        True,
                        f"An incomplete developer installation of Sekka is on this Mac ({name}): {serial}, but "
                        f"{missing}. The installer won't repair it or install beside it. {remove}",
                    )
                else:
                    self.issue(
                        "existing-sekka",
                        True,
                        f"A developer installation of Sekka, as install-sekka.sh makes, is on this Mac ({name}): "
                        f"{serial}. The installer won't install beside it or change it. {remove}",
                    )
            return {
                "kind": "sekka",
                "name": name,
                "state": state,
                "stub_group": stub_group,
                "esp": esp,
                "root": root,
                "installation_id": None,
                "developer": developer,
            }
        asahi_markers = bool(o.m1n1_ver) or os.path.isdir(os.path.join(o.system or "", ASAHI_STEP2_APP))
        if not asahi_markers and esp_mount:
            asahi_markers = os.path.isdir(os.path.join(esp_mount, "asahi"))
        if asahi_markers:
            state = self.asahi_state(o, bp_known, enrolled)
            kind = "other-linux"
        else:
            kind, state = "unknown", "unknown"
            self.issue(
                "unrecognized-layout",
                False,
                f"{name} is a startup volume for another system the installer doesn't recognize. "
                "It will be left as it is.",
            )
        return {
            "kind": kind,
            "name": name,
            "state": state,
            "stub_group": stub_group,
            "esp": esp,
            "root": root,
            "installation_id": None,
            "developer": False,
        }

    def orphan(self, p, index, attached=()):
        """An EFI or Linux partition that no startup volume claims, and the
        partitions it claims with it: a developer ESP the Linux partitions
        directly after it, which install-sekka.sh's Asahi installer put
        there and which macOS can't read."""
        handoff = files = None
        developer = False
        claimed = []
        if p.type == "EFI":
            mountpoint = self.esp_mount(p)
            raw = self.read_mounted(mountpoint, c.HANDOFF_PATH)
            if raw is not None:
                try:
                    handoff = c.parse_handoff(raw)
                except (c.Invalid, canonical.CanonicalError, KeyError, TypeError):
                    handoff = None
            files = self.boot_files(mountpoint)
            # A handoff names the installation, whatever its serial.
            developer = not handoff and has_developer_serial(p)
        is_efi = p.type == "EFI"
        sekka = bool(handoff) or developer or bool(files and files[BOOT_BIN_RECORD])
        if handoff:
            self.issue(
                "partial-installation",
                True,
                f"{p.name} holds Sekka's boot files but no startup volume goes with it: an "
                "installation was interrupted or partly removed. Nothing will be changed.",
            )
        elif developer:
            claimed = self.linux_after(index, attached)
            self.issue(
                "partial-installation",
                True,
                f"The EFI partition {p.name} has the developer image's serial, {DEVELOPER_ESP_NAME}, but no "
                "startup volume goes with it: a developer installation of Sekka was interrupted or partly "
                f"removed. The installer won't change it or install beside it. {removal(None, p, claimed)}",
            )
        elif sekka:
            self.issue(
                "partial-installation",
                True,
                f"{p.name} holds Sekka's boot record ({BOOT_BIN_RECORD}) but no startup volume goes with it, "
                "and neither the installer's records nor the developer image's serial say which installation "
                "it was. Nothing will be changed.",
            )
        else:
            self.issue(
                "unrecognized-layout",
                False,
                f"{p.name} ({p.type}) belongs to no system the installer recognizes. It will be "
                "left as it is.",
            )
        root = p if not is_efi else claimed[0] if claimed else None
        return {
            "kind": "sekka" if sekka else "unknown",
            "name": c.plain_text((p.info or {}).get("VolumeName") or p.name, limit=255, keep="start"),
            "state": "partial",
            "stub_group": None,
            "esp": c.uuid_any(p.uuid) if is_efi else None,
            "root": c.uuid_any(root.uuid) if root is not None else None,
            "installation_id": handoff["installation"] if handoff else None,
            "developer": developer,
        }, claimed

    # Candidates --------------------------------------------------------------

    def candidates(self, containers):
        v = self.view
        main = self.a.main
        found = []
        for p in v.parts:
            if p.free and p.size >= main.STUB_SIZE:
                found.append({"kind": "free-space", "extent": {"offset": p.offset, "size": p.size}})
        by_partition = {k["partition"]: k for k in containers}
        for i, p in enumerate(v.parts):
            if p.free or p.type != "Apple_APFS" or p.container is None or not self.holds_macos(p):
                continue
            k = by_partition.get(c.uuid_any(p.uuid))
            if k is None or k["resize"] is None:
                continue
            # Asahi's can_resize: free space beyond macOS's reserve and 500 MB.
            if p.container["CapacityFree"] <= self.min_free(p) + self.a.util.psize("500MB"):
                continue
            available = k["size"] - k["resize"]["minimum"]
            if available < 2 * main.PART_ALIGN:
                continue
            nxt = v.parts[i + 1] if i + 1 < len(v.parts) else None
            adjacent = nxt.size if nxt is not None and nxt.free and nxt.offset == p.offset + p.size else 0
            found.append(
                {
                    "kind": "shrink-container",
                    "container": k["uuid"],
                    "partition": k["partition"],
                    "current_size": k["size"],
                    "available": available,
                    "available_recommended": k["size"] - k["resize"]["recommended"],
                    "adjacent_free": adjacent,
                }
            )
        found.sort(key=c.candidate_capacity, reverse=True)
        return found

    # Checks ------------------------------------------------------------------

    def machine_issues(self, machine, macos_version):
        s = self.view.sysinfo
        main = self.a.main
        facts = {
            "product_type": machine["product_type"],
            "device_class": machine["device_class"],
            "chip_id": machine["chip_id"],
            "system_firmware": machine["system_firmware"],
            "macos": macos_version,
        }
        refusals, acks = c.admit(self.profile, facts)
        self.admission = (refusals, acks)
        if main.DEVICES.get(s.device_class) is None or main.CHIP_MIN_VER.get(s.chip_id) is None:
            self.issue(
                "unsupported-machine",
                True,
                f"Asahi's installer doesn't support this Mac ({s.product_type}, {s.device_class}).",
            )
        for kind, details in refusals + acks:
            blocking = (kind, details) in refusals
            if kind in ("unknown-machine", "chip-mismatch"):
                self.issue(
                    "unsupported-machine",
                    True,
                    f"This release is built for the {self.profile['name']} only. This Mac is a "
                    f"{s.product_name} ({s.product_type}, {s.device_class}, chip {s.chip_id:#x}).",
                )
            elif kind == "untested-system-firmware":
                self.issue(
                    "untested-firmware",
                    blocking,
                    f"This Mac's system firmware is {details['found']}; Sekka's boot software has "
                    f"been tested on {', '.join(details['tested'])} only.",
                )
            elif kind == "macos-too-old":
                self.issue(
                    "macos-too-old",
                    True,
                    f"macOS {details['found']} is too old; update to macOS {details['minimum']} or later.",
                )
            elif kind == "known-issue":
                self.issue("known-macos-issue", blocking, f"{details['summary']} ({details['reference']})")
        if c.version_parts(macos_version) < c.version_parts(main.MIN_MACOS_VERSION):
            self.issue(
                "macos-too-old",
                True,
                f"Asahi's installer needs macOS {main.MIN_MACOS_VERSION} or later; this Mac runs {macos_version}.",
            )
        holder = type("Main", (), {"sysinfo": s})()
        try:
            self.a.bugs.run_checks(holder)
        except SystemExit:
            self.issue(
                "known-macos-issue",
                True,
                "Asahi's installer refuses this combination of a ProMotion display and macOS or system "
                "firmware older than 14.2: it can leave the Mac unable to start. Update macOS first.",
            )

    def boot_issues(self, cur_os):
        s = self.view.sysinfo
        if cur_os is None:
            self.issue(
                "not-normal-boot",
                True,
                "The installer couldn't find the macOS it is running from on the internal disk. "
                "Start up from the internal disk's macOS and try again.",
            )
            return False
        if s.boot_mode == "macOS":
            return True
        if s.boot_mode == "Unknown" and not self.privileged:
            # bputil needs root; the booted volume group is the running macOS.
            self.unavailable.append("boot policy (bputil needs administrator rights)")
            return True
        self.issue(
            "not-normal-boot",
            True,
            f"This Mac is running {s.boot_mode}, not its normal macOS. Start up from the internal "
            "disk's macOS and try again.",
        )
        return False

    def power_issues(self, power):
        if power is None:
            self.issue("on-battery", True, "The installer couldn't tell whether this Mac is on its charger. Connect it.")
            return {"has_battery": True, "on_ac": False, "battery_percent": None}
        if power["has_battery"] and not power["on_ac"]:
            self.issue("on-battery", True, "Connect this Mac to its charger: installing mustn't run on battery.")
        pct = power["battery_percent"]
        if power["has_battery"] and pct is not None and pct < 20:
            self.issue("battery-low", False, f"The battery is at {pct}%. Let it charge a little before installing.")
        return power

    def disk_issues(self, doc, candidates):
        v = self.view
        internal = 0
        for name, dsk in v.dutil.disks.items():
            try:
                if dsk.get("VirtualOrPhysical") == "Virtual" or not dsk.get("Internal"):
                    continue
                if v.dutil.disk_parts[name]["Partitions"][0]["Content"] == "Apple_APFS_ISC":
                    internal += 1
            except (KeyError, IndexError):
                continue
        if internal != 1:
            self.issue(
                "ambiguous-disk",
                True,
                f"The installer found {internal} internal disks with Apple's system container; it "
                "installs only where there is exactly one.",
            )
        if not any(c.candidate_capacity(k) > self.a.main.MIN_INSTALL_FREE for k in candidates):
            self.issue(
                "not-enough-space",
                True,
                "There is neither enough free space nor a macOS container that can shrink far enough "
                "for Sekka.",
            )
        for k in doc["containers"]:
            if self.holds_macos(v.partition(k["partition"])):
                if k["resize"] is None:
                    self.issue(
                        "resize-limits-unavailable",
                        False,
                        "diskutil couldn't say how far the macOS container can shrink, so shrinking "
                        "isn't offered.",
                    )
                if k["snapshots"]:
                    self.issue(
                        "local-snapshots",
                        False,
                        f"macOS keeps {k['snapshots']} local snapshot(s), which can stop Apple's resize. "
                        "The installer never deletes them; Time Machine's can be removed in its settings.",
                    )
        if any(kind == "snapshots" for kind, _ in self.notes):
            self.issue(
                "local-snapshots",
                False,
                "The installer couldn't list the macOS volume's snapshots; if a resize fails, local "
                "Time Machine snapshots are a known cause.",
            )

    # The whole ------------------------------------------------------------------

    def build(self):
        v = self.view
        s = v.sysinfo
        machine = self.machine()
        cur_os = v.current_os()
        macos_version = parse_version(s.macos_ver)
        sfr_version = parse_version(s.sfr_ver)
        if macos_version is None or not s.macos_build:
            raise InspectionError("macOS's version couldn't be read")
        if sfr_version is None:
            raise InspectionError("the system firmware's macOS version (SFR) couldn't be read")
        if cur_os is not None and cur_os.partition is not None:
            if cur_os.partition.name.rsplit("s", 1)[0] != v.sys_disk:
                cur_os = None
        self.machine_issues(machine, macos_version)
        normal = self.boot_issues(cur_os)
        data_volume = None
        if cur_os is not None:
            for vol in cur_os.partition.container.get("Volumes", []):
                if vol.get("Roles") == ["Data"] and vol.get("DeviceIdentifier") == cur_os.data_volume:
                    data_volume = vol
        disk = self.disk()
        candidates = self.candidates(disk["containers"])
        installations = self.installations(cur_os)
        power = self.power_issues(read_power(self.host))
        filevault = read_filevault(self.host, data_volume)
        if filevault == "converting":
            self.issue(
                "file-vault-converting",
                True,
                "FileVault is turning on or off. Wait until it finishes; resizing waits for it.",
            )
        elif filevault == "unknown":
            self.unavailable.append("FileVault status")
        self.disk_issues(disk, candidates)
        doc = {
            "schema": c.INSPECTION_SCHEMA,
            "machine": machine,
            "macos": {
                "version": macos_version,
                "build": c.text(s.macos_build),
                "system_firmware_version": sfr_version,
                "volume_group": c.uuid_any(cur_os.vgid) if cur_os is not None else c.uuid_any(s.boot_uuid),
                "internal_boot": bool(normal and cur_os is not None),
            },
            "power": power,
            "filevault": filevault,
            "disk": disk,
            "layout_digest": c.layout_digest(disk),
            "candidates": candidates,
            "installations": installations,
            "issues": self.issues,
        }
        c.check_inspection(doc)
        if len(canonical.dumps(doc)) > c.LIMIT_INSPECTION:
            raise InspectionError("the inspection is larger than the contract allows")
        self.cur_os = cur_os
        return doc


def inspect_mac(asahi, host, profile, privileged):
    """Inspect with Asahi's model. Returns (inspection, inspector).

    As root or not, every volume it mounts is mounted read-only and unmounted
    again before it returns; `privileged` only says what it may expect to
    read (boot policies, the admin-user records). The inspector's model stays
    usable as data afterwards, but not its mount points."""
    from .asahi import View

    view = View(asahi)
    try:
        inspector = Inspector(asahi, host, profile, view, privileged)
        doc = inspector.build()
    finally:
        view.close()
    return doc, inspector
