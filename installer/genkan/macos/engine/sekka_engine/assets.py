# SPDX-License-Identifier: GPL-3.0-or-later
"""The release's files, checked against the plan before anything changes.

The assets directory holds exactly what the helper imported and verified:

    payload.zip           the OS package: esp/ and root.img (stored)
    installer_data.json   Asahi's installer data for this release
    manifest.json         the image manifest (sekka.mac.manifest/1)
    recovery.zip          Sekka Recovery.app, as a zip

Each is opened without following links, and its size and SHA-256 must be the
plan's. The engine then checks what is inside: Asahi's template describes an
ESP and a root image and nothing else, every ESP file is the manifest's,
byte for byte, and the Recovery app's zip holds the app and nothing else.
"""

import hashlib
import json
import os
import stat
import zipfile

from . import canonical
from . import contract as c

ASSETS = {
    "payload": "payload.zip",
    "installer_data": "installer_data.json",
    "manifest": "manifest.json",
    "recovery_app": "recovery.zip",
}
MANIFEST_SCHEMA = "sekka.mac.manifest/1"
LIMIT_MANIFEST = 4 * 1024 * 1024
LIMIT_INSTALLER_DATA = 1024 * 1024

RECOVERY_APP = "Sekka Recovery.app"
RECOVERY_MAX_ENTRIES = 4096
RECOVERY_MAX_BYTES = 512 * 1024 * 1024
ESP_PREFIX = "esp/"
ROOT_IMAGE = "root.img"


class AssetError(ValueError):
    """An asset differs from what the plan names, or is unsafe to use."""


def open_regular(path):
    """Open a file without following a link at its last component, and
    require a regular file."""
    flags = os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0) | getattr(os, "O_CLOEXEC", 0)
    try:
        fd = os.open(path, flags)
    except OSError as error:
        raise AssetError(f"{os.path.basename(path)} can't be opened: {error.strerror}") from error
    st = os.fstat(fd)
    if not stat.S_ISREG(st.st_mode):
        os.close(fd)
        raise AssetError(f"{os.path.basename(path)} isn't a regular file")
    return os.fdopen(fd, "rb")


def hash_open(f, chunk=8 * 1024 * 1024):
    h = hashlib.sha256()
    size = 0
    while True:
        block = f.read(chunk)
        if not block:
            break
        h.update(block)
        size += len(block)
    return size, "sha256:" + h.hexdigest()


def verify_assets(directory, plan):
    """Every asset's digest against the plan. Returns {role: path}."""
    paths = {}
    st = os.lstat(directory)
    if not stat.S_ISDIR(st.st_mode):
        raise AssetError("the assets directory isn't a directory")
    for role, name in ASSETS.items():
        path = os.path.join(directory, name)
        with open_regular(path) as f:
            _, found = hash_open(f)
        if found != plan["release"][role]:
            raise AssetError(f"{name} isn't the file this plan was made for ({found})")
        paths[role] = path
    return paths


# Asahi's installer data ------------------------------------------------------

TEMPLATE_KEYS = {
    "name",
    "default_os_name",
    "boot_object",
    "next_object",
    "package",
    "supported_fw",
    "partitions",
    "expert",
    "external_boot",
}
ESP_KEYS = {"name", "type", "size", "format", "volume_id", "copy_firmware", "copy_installer_data", "source"}
ROOT_KEYS = {"name", "type", "size", "expand", "image"}


def load_template(path, util, plan, os_firmware):
    """The one OS in installer_data.json, checked to be exactly the ESP and
    root image Sekka's payload has. `util` is Asahi's (for psize/align_up)."""
    with open_regular(path) as f:
        data = f.read(LIMIT_INSTALLER_DATA + 1)
    if len(data) > LIMIT_INSTALLER_DATA:
        raise AssetError("installer_data.json is too large")
    try:
        doc = json.loads(data)
    except ValueError as error:
        raise AssetError("installer_data.json isn't JSON") from error
    if not isinstance(doc, dict) or set(doc) != {"os_list"} or not isinstance(doc["os_list"], list):
        raise AssetError("installer_data.json has an os_list and nothing else")
    if len(doc["os_list"]) != 1:
        raise AssetError("installer_data.json must offer exactly one OS")
    t = doc["os_list"][0]
    if not isinstance(t, dict) or set(t) - TEMPLATE_KEYS:
        raise AssetError("the OS template has fields Sekka doesn't use (extras, icons, ...)")
    if t.get("expert") or t.get("external_boot"):
        raise AssetError("the OS template is for expert or external installs")
    if t.get("boot_object") != "m1n1.bin" or t.get("next_object") != "m1n1/boot.bin":
        raise AssetError("the OS template must chainload m1n1/boot.bin with Asahi's m1n1.bin")
    if os_firmware not in (t.get("supported_fw") or []):
        raise AssetError(f"the OS template doesn't support OS firmware {os_firmware}")
    parts = t.get("partitions")
    if not isinstance(parts, list) or len(parts) != 2:
        raise AssetError("the OS template has exactly an ESP and a root partition")
    esp, root = parts
    if set(esp) - ESP_KEYS or set(root) - ROOT_KEYS:
        raise AssetError("a partition template has unknown fields")
    if (
        esp.get("type") != "EFI"
        or esp.get("format") != "fat"
        or esp.get("copy_firmware") is not True
        or esp.get("source") != "esp"
    ):
        raise AssetError("the first partition must be a FAT ESP from esp/, with Apple's firmware")
    if root.get("type") != "Linux" or root.get("expand") is not True or root.get("image") != ROOT_IMAGE:
        raise AssetError("the second partition must be the expanding root, from root.img")
    esp_size = util.align_up(util.psize(esp["size"]), c.ALIGN)
    root_min = util.align_up(util.psize(root["size"]), c.ALIGN)
    if esp_size != plan["allocation"]["esp"]:
        raise AssetError(f"the template's ESP is {esp_size} bytes; the plan's is {plan['allocation']['esp']}")
    if root_min > plan["allocation"]["root"]:
        raise AssetError("the plan's root partition is smaller than the root image")
    return doc, t


# The image manifest -----------------------------------------------------------


def load_manifest(path, plan):
    """sekka.mac.manifest/1: the root image and every ESP file, by size and
    digest. Extra top-level fields (store and source details) are allowed;
    these must be present and agree with the plan."""
    with open_regular(path) as f:
        data = f.read(LIMIT_MANIFEST + 1)
    try:
        doc = canonical.loads_relaxed(data, LIMIT_MANIFEST, MANIFEST_SCHEMA)
    except canonical.CanonicalError as error:
        raise AssetError(f"manifest.json: {error}") from error
    try:
        if doc["release"] != plan["release"]["release"]:
            raise AssetError("the manifest is for another release")
        if doc["source_revision"] != plan["release"]["source_revision"]:
            raise AssetError("the manifest was built from another revision")
        root = c.fields(doc["root_image"], ("path", "size", "digest"))
        if root["path"] != ROOT_IMAGE or root["digest"] != plan["release"]["root_image"]:
            raise AssetError("the manifest's root image isn't the plan's")
        c.u64(root["size"])
        if root["size"] == 0 or root["size"] % 4096:
            raise AssetError("the root image's size isn't a multiple of 4 KiB")
        esp = c.fields(doc["esp"], ("boot_entry", "files"))
        files = {}
        for item in esp["files"]:
            c.fields(item, ("path", "size", "digest"))
            check_relative(item["path"])
            c.u64(item["size"])
            c.digest(item["digest"])
            key = item["path"].casefold()
            if key in {k.casefold() for k in files}:
                raise AssetError(f"the ESP lists {item['path']} twice")
            files[item["path"]] = item
        if esp["boot_entry"] not in files:
            raise AssetError("the manifest's boot entry isn't one of its ESP files")
        reserved = {c.HANDOFF_PATH.casefold()}
        if any(p.casefold() in reserved or p.casefold().startswith("vendorfw/") for p in files):
            raise AssetError("the ESP files collide with what the engine writes")
    except (KeyError, TypeError, c.Invalid) as error:
        raise AssetError(f"manifest.json is malformed: {error}") from error
    return doc, files


def check_relative(path, allow_dir=False):
    if not isinstance(path, str) or not path or "\\" in path or "\0" in path or path.startswith("/"):
        raise AssetError(f"not a plain relative path: {path!r}")
    parts = path.split("/")
    if allow_dir and parts[-1] == "":
        parts = parts[:-1]
    if not parts or any(p in ("", ".", "..") for p in parts) or any(len(p.encode()) > 255 for p in parts):
        raise AssetError(f"not a plain relative path: {path!r}")
    return parts


def member_kind(info):
    """'dir', 'file', or None for anything else (links, devices)."""
    mode = info.external_attr >> 16
    if info.filename.endswith("/"):
        if mode and not stat.S_ISDIR(mode):
            return None
        return "dir"
    if mode == 0 or stat.S_ISREG(mode):
        return "file"
    return None


def hash_member(zf, info, chunk=8 * 1024 * 1024):
    h = hashlib.sha256()
    size = 0
    with zf.open(info) as f:  # reading to the end checks the CRC
        while True:
            block = f.read(chunk)
            if not block:
                break
            h.update(block)
            size += len(block)
    return size, "sha256:" + h.hexdigest()


def open_zip(path):
    f = open_regular(path)
    try:
        return zipfile.ZipFile(f)
    except (zipfile.BadZipFile, OSError, ValueError) as error:
        f.close()
        raise AssetError(f"{os.path.basename(path)} isn't a zip: {error}") from error


def close_zip(zf):
    """Close a ZipFile and the file it was given (ZipFile leaves that open)."""
    fp = zf.fp
    zf.close()
    if fp is not None:
        fp.close()


def check_payload(path, manifest, esp_files):
    """The OS package holds esp/ (exactly the manifest's files) and
    root.img, nothing else; every ESP file's bytes are the manifest's."""
    zf = open_zip(path)
    try:
        return _check_payload(zf, manifest, esp_files)
    except BaseException:
        close_zip(zf)
        raise


def _check_payload(zf, manifest, esp_files):
    seen = set()
    esp_found = {}
    root = None
    for info in zf.infolist():
        name = info.filename
        if info.flag_bits & 0x1:
            raise AssetError(f"payload member {name} is encrypted")
        key = name.casefold()
        if key in seen:
            raise AssetError(f"payload member {name} appears twice")
        seen.add(key)
        kind = member_kind(info)
        if kind is None:
            raise AssetError(f"payload member {name} is a link or device")
        if name == ROOT_IMAGE and kind == "file":
            root = info
            continue
        if not name.startswith(ESP_PREFIX):
            raise AssetError(f"payload member {name} is outside esp/ and root.img")
        rel = name[len(ESP_PREFIX) :]
        if rel == "":
            continue
        check_relative(rel, allow_dir=(kind == "dir"))
        if kind == "file":
            esp_found[rel] = info
    if root is None:
        raise AssetError("the payload has no root.img")
    if root.file_size != manifest["root_image"]["size"]:
        raise AssetError("root.img's size isn't the manifest's")
    if set(esp_found) != set(esp_files):
        missing = sorted(set(esp_files) - set(esp_found))[:5]
        extra = sorted(set(esp_found) - set(esp_files))[:5]
        raise AssetError(f"the payload's ESP differs from the manifest (missing {missing}, extra {extra})")
    for rel, info in sorted(esp_found.items()):
        size, found = hash_member(zf, info)
        if size != esp_files[rel]["size"] or found != esp_files[rel]["digest"]:
            raise AssetError(f"esp/{rel} isn't the manifest's file")
    entry = zf.read(ESP_PREFIX + manifest["esp"]["boot_entry"]).decode("utf-8")
    c.fill_entry(entry, "00000000-0000-4000-8000-000000000000", "00000000-0000-4000-8000-000000000001")
    return zf


# The Recovery app ---------------------------------------------------------------


def check_recovery_zip(path):
    """Every member of recovery.zip: a plain relative path inside
    `Sekka Recovery.app/`, a regular file or directory, stored or deflated,
    not encrypted, no name twice (APFS ignores case), no links, no
    `__MACOSX` resource forks, and the app's Info.plist and executable.
    Returns (zipfile, [(info, kind, mode, size, digest)])."""
    zf = open_zip(path)
    try:
        return zf, _check_recovery_members(zf)
    except BaseException:
        close_zip(zf)
        raise


def _check_recovery_members(zf):
    infos = zf.infolist()
    if not infos or len(infos) > RECOVERY_MAX_ENTRIES:
        raise AssetError("recovery.zip has no members or too many")
    seen = set()
    members = []
    total = 0
    has_plist = has_exec = False
    for info in infos:
        name = info.filename
        if info.flag_bits & 0x1:
            raise AssetError(f"{name} is encrypted")
        if info.compress_type not in (zipfile.ZIP_STORED, zipfile.ZIP_DEFLATED):
            raise AssetError(f"{name} uses an unexpected compression")
        kind = member_kind(info)
        if kind is None:
            raise AssetError(f"{name} is a link or a device, not a file")
        parts = check_relative(name, allow_dir=(kind == "dir"))
        if parts[0] != RECOVERY_APP:
            raise AssetError(f"{name} is outside {RECOVERY_APP}")
        if any(p == "__MACOSX" or p.startswith("._") for p in parts):
            raise AssetError(f"{name} is resource-fork residue")
        key = "/".join(parts).casefold()
        if key in seen:
            raise AssetError(f"{name} appears twice")
        seen.add(key)
        mode = info.external_attr >> 16
        if kind == "dir":
            members.append((info, kind, 0o755, 0, None))
            continue
        total += info.file_size
        if total > RECOVERY_MAX_BYTES:
            raise AssetError("recovery.zip expands to more than 512 MiB")
        size, found = hash_member(zf, info)
        if size != info.file_size:
            raise AssetError(f"{name} isn't the size its entry says")
        members.append((info, kind, 0o755 if mode & 0o111 else 0o644, size, found))
        rest = "/".join(parts[1:])
        if rest == "Contents/Info.plist":
            has_plist = True
        if len(parts) == 4 and parts[1:3] == ["Contents", "MacOS"] and mode & 0o111:
            has_exec = True
    if not has_plist or not has_exec:
        raise AssetError(f"recovery.zip doesn't hold a complete {RECOVERY_APP}")
    return members


def recovery_inventory(members):
    """The digest of the app's files (path, size, digest), sorted."""
    files = [
        {"path": "/".join(check_relative(i.filename)), "size": size, "digest": d}
        for i, kind, mode, size, d in members
        if kind == "file"
    ]
    files.sort(key=lambda f: f["path"].encode("utf-8"))
    return canonical.digest({"domain": "sekka.mac.recovery-app/1", "files": files}), files


def esp_inventory_digest(files):
    """The digest of an ESP inventory: every file's path, size and digest,
    sorted by path. The image producer's `image.esp_inventory` should use
    the same form."""
    items = sorted(
        ({"path": f["path"], "size": f["size"], "digest": f["digest"]} for f in files),
        key=lambda f: f["path"].encode("utf-8"),
    )
    return canonical.digest({"domain": "sekka.mac.esp-inventory/1", "files": items})
