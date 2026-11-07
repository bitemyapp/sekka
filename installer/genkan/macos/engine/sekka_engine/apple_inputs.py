# SPDX-License-Identifier: GPL-3.0-or-later
"""Apple's restore-image parts for the stub, fetched before any disk changes.

Asahi's StubInstaller opens Apple's macOS 14.8.3 update (a zip on
updates.cdn-apple.com) through its URLCache and reads the members it needs
while it installs the stub, so a network failure there leaves a half-built
stub. Here the same members are fetched first, in the AppleInputs stage,
into a private spool file at their own offsets: the spool is a sparse local
copy of the remote zip that holds exactly the fetched ranges, and reading
anything else from it is an error, never silent zeros. The stub install then
reads the spool through an ordinary ZipFile.

What the bytes' authenticity rests on is what Asahi's rests on: an HTTPS
connection to the host in the URL Asahi v0.9.2 pins (main.IPSW_VERSIONS),
verified against the CA bundle in the pinned runtime, and the zip's own
CRC-32 for each member, checked as each member is read in full. Neither
Asahi nor this adapter checks a cryptographic digest of Apple's bytes; the
Mac's boot chain verifies the signed boot objects it later loads, and does
not cover the firmware copied out for Linux.
"""

import bisect
import hashlib
import logging
import os
import struct
import types
import zipfile

log = logging.getLogger("sekka_engine.apple_inputs")

CHUNK = 8 * 1024 * 1024
# A zip's end record is within the last 64 KiB + 22 bytes.
EOCD_WINDOW = 65536 + 22
PLACEHOLDER_VGID = "00000000-0000-0000-0000-000000000000"


class NoRoom(Exception):
    """Apple's files would need more room than the plan left for them."""

    def __init__(self, needed, room):
        super().__init__(f"Apple's files need {needed} bytes; the plan leaves {room}")
        self.needed = needed
        self.room = room


class SpoolGap(IOError):
    """A read from the spool reached bytes that were never fetched."""


class Spool:
    """A sparse file with the same offsets as the remote zip, and the list of
    ranges that hold fetched bytes."""

    def __init__(self, path, size):
        self.path = path
        self.size = size
        self.ranges = []  # sorted, disjoint [start, end)
        self.fd = os.open(path, os.O_RDWR | os.O_CREAT | os.O_TRUNC | getattr(os, "O_CLOEXEC", 0), 0o600)
        os.ftruncate(self.fd, size)
        self.p = 0
        self.fetched = 0

    def _add(self, start, end):
        merged = []
        for s, e in self.ranges:
            if e < start or s > end:
                merged.append((s, e))
            else:
                start, end = min(s, start), max(e, end)
        merged.append((start, end))
        merged.sort()
        self.ranges = merged

    def covered(self, start, end):
        if start >= end:
            return True
        i = bisect.bisect_right(self.ranges, (start, float("inf"))) - 1
        return i >= 0 and self.ranges[i][0] <= start and self.ranges[i][1] >= end

    def fetch(self, source, start, end, progress=None):
        """Copy [start, end) from `source` (a URLCache) into the spool."""
        end = min(end, self.size)
        pos = start
        while pos < end:
            if self.covered(pos, min(end, pos + CHUNK)):
                pos = min(end, pos + CHUNK)
                continue
            n = min(CHUNK, end - pos)
            source.seek(pos)
            data = source.read(n)
            if len(data) != n:
                raise IOError("Apple's server returned a short read")
            os.pwrite(self.fd, data, pos)
            self.fetched += n
            self._add(pos, pos + n)
            # URLCache keeps every block it has read; the spool holds them now.
            if hasattr(source, "cache"):
                source.cache.clear()
            pos += n
            if progress:
                progress(n)

    # A read-only file object for zipfile.

    def seekable(self):
        return True

    def readable(self):
        return True

    def seek(self, offset, whence=os.SEEK_SET):
        if whence == os.SEEK_SET:
            self.p = offset
        elif whence == os.SEEK_CUR:
            self.p += offset
        elif whence == os.SEEK_END:
            self.p = self.size + offset
        return self.p

    def tell(self):
        return self.p

    def read(self, count=-1):
        if count is None or count < 0:
            count = self.size - self.p
        count = max(0, min(count, self.size - self.p))
        if not self.covered(self.p, self.p + count):
            raise SpoolGap(f"bytes {self.p}-{self.p + count} of Apple's restore image weren't fetched")
        data = os.pread(self.fd, count, self.p)
        self.p += len(data)
        return data

    def close(self):
        if self.fd is not None:
            os.close(self.fd)
            self.fd = None


def needed_members(ins, identity, zf):
    """The members Asahi's StubInstaller reads for this identity: the ones
    install_files() extracts and collect_firmware() takes, by the same rules
    and through the same path mapping (stub.py, v0.9.2)."""
    names = set()
    listing = zf.namelist()

    def one(path):
        names.add(ins.path(path))

    def tree(path):
        prefix = ins.path(path)
        if not prefix.endswith("/"):
            prefix += "/"
        names.update(n for n in listing if n.startswith(prefix))

    for path in (
        "BuildManifest.plist",
        "usr/standalone/bootcaches.plist",
        "SystemVersion.plist",
        "PlatformSupport.plist",
        "RestoreVersion.plist",
        "BootabilityBundle/Restore/Firmware/Bootability.dmg.trustcache",
    ):
        one(path)
    tree("BootabilityBundle/Restore/Bootability")
    tree(f"Firmware/Manifests/restore/{ins.variant}/")
    for key, val in identity["Manifest"].items():
        info = val["Info"]
        path = info["Path"]
        install = key not in (
            "BaseSystem",
            "OS",
            "Ap,SystemVolumeCanonicalMetadata",
            "RestoreRamDisk",
            "RestoreTrustCache",
        ) and not key.startswith("Cryptex")
        fud = (
            key not in ("BaseSystem", "OS", "Ap,SystemVolumeCanonicalMetadata", "StaticTrustCache", "SystemVolume")
            and info.get("IsFUDFirmware", False)
            and not info.get("IsLoadedByiBoot", False)
            and not info.get("IsLoadedByiBootStage1", False)
            and path.endswith(".im4p")
        )
        if install or fud:
            one(path)
    if ins.is_ota:
        names.add("AssetData/payloadv2/basesystem_patches/arm64eBaseSystem.dmg")
    else:
        one(identity["Manifest"]["BaseSystem"]["Info"]["Path"])
    missing = sorted(n for n in names if n not in set(listing))
    if missing:
        raise KeyError(f"Apple's restore image lacks {', '.join(missing[:5])}")
    return sorted(names)


def member_range(source, info):
    """[start, end) of a member's local header, name, extra field and data."""
    source.seek(info.header_offset)
    header = source.read(zipfile.sizeFileHeader)
    if len(header) != zipfile.sizeFileHeader or header[:4] != zipfile.stringFileHeader:
        raise zipfile.BadZipFile(f"{info.filename}: bad local header")
    fields = struct.unpack(zipfile.structFileHeader, header)
    name_len = fields[zipfile._FH_FILENAME_LENGTH]
    extra_len = fields[zipfile._FH_EXTRA_FIELD_LENGTH]
    start = info.header_offset
    return start, start + zipfile.sizeFileHeader + name_len + extra_len + info.compress_size


def prepare(asahi, ins, ipsw, workdir, progress=None, room=None):
    """Open Apple's image with Asahi's StubInstaller.load_ipsw, choose the
    build identity with its load_identity, and spool every member the stub
    needs. Afterwards `ins` reads from the spool, not the network.

    The spool lives on macOS's volume until the run ends. When `room` is
    given and the members would take more than that, NoRoom is raised after
    reading only the zip's directory, before the members are fetched.

    Returns facts for the checkpoint."""
    ins.load_ipsw(ipsw)
    source = ins.ucache
    remote = ins.pkg
    # load_identity computes the stub's paths from its OSInfo, which doesn't
    # exist until the stub partition does; a placeholder serves until then.
    ins.osi = types.SimpleNamespace(system="/", preboot="/", vgid=PLACEHOLDER_VGID)
    identity = ins.load_identity()
    with remote.open(ins.path("BuildManifest.plist")) as f:
        manifest_bytes = f.read()
    names = needed_members(ins, identity, remote)
    if source is None:
        raise IOError("Apple's restore image must come from Apple's server")
    start_dir = getattr(remote, "start_dir", None)
    tail = max(0, source.size - EOCD_WINDOW)
    if isinstance(start_dir, int) and 0 <= start_dir < source.size:
        tail = min(tail, start_dir)
    ranges = [(tail, source.size)]
    for name in names:
        ranges.append(member_range(source, remote.getinfo(name)))
    total = sum(e - s for s, e in ranges)
    # Each range may round up to a whole 4 KiB block once written.
    needed = total + 4096 * len(ranges)
    if room is not None and needed > room:
        source.close_connection()
        raise NoRoom(needed, room)
    spool = Spool(os.path.join(workdir, "restore.spool"), source.size)
    for s, e in ranges:
        spool.fetch(source, s, e, progress=(lambda n: progress(n, total)) if progress else None)
    local = zipfile.ZipFile(spool)
    sizes = 0
    for name in names:
        info = local.getinfo(name)
        with local.open(info) as f:  # read to the end: the CRC-32 is checked
            while f.read(CHUNK):
                pass
        sizes += info.file_size
    source.close_connection()
    ins.pkg = local
    ins.ucache = None
    ins.osi = None
    return {
        "identity": identity,
        "build": identity["Info"]["BuildNumber"],
        "manifest_digest": "sha256:" + hashlib.sha256(manifest_bytes).hexdigest(),
        "members": len(names),
        "member_bytes": sizes,
        "fetched": spool.fetched,
        "remote_size": source.size,
        "url": ipsw.url,
        "host": source.url.netloc if hasattr(source, "url") else "",
        "spool": spool,
    }
