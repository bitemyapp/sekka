"""Write a Mac release's manifest (sekka.mac.manifest/1), after checking the
payload against what it is built from.

The manifest is canonical JSON as the installer contract defines it
(installer/genkan/macos, canonical.rs): keys in byte order, no whitespace,
integers only. Its `source` (repository, revision, lock), `image` (ImageFacts)
and `allocation` (AllocationPolicy) objects have the shapes the catalog's
Release takes them in (catalog.rs); the rest says how they were derived.
docs/apple-silicon/image.md describes every field.

Checks, failing the build:
- the payload zip holds exactly esp/ (every file of the ESP inventory, with
  its size and digest) and root.img (the root image's size and digest);
- installer_data.json names the payload and the ESP and root sizes;
- the ESP has room for the configured number of generations, the firmware
  archive and boot.bin's previous copy;
- the numbers satisfy the catalog's own consistency checks.
"""

import argparse
import hashlib
import json
import os
import sys
import zipfile
from pathlib import Path

SCHEMA = "sekka.mac.manifest/1"
MIB = 1024 * 1024
GIB = 1024 * MIB
# Yukimi doesn't start an update with less free space than this
# (DEVELOPERS.md, installed-system updates).
UPDATE_FLOOR = 10_000_000_000
# Room for the owner's files in the recommended allocation.
DATA_ROOM = 64 * GIB
# Apple's firmware archive the installer extracts (vendorfw/firmware.cpio)
# is tens of MiB on an M3; allow for growth.
FIRMWARE_ALLOWANCE = 128 * MIB
PLACEHOLDERS = ["@SEKKA_ROOT_PARTUUID@", "@SEKKA_ESP_PARTUUID@"]
ENTRY = "loader/entries/nixos-generation-1.conf"
BOOT_FILES = [
    ("m1n1-stage2", "m1n1/boot.bin"),
    ("removable-loader", "EFI/BOOT/BOOTAA64.EFI"),
    ("systemd-boot", "EFI/systemd/systemd-bootaa64.efi"),
    ("kernel", "EFI/nixos/kernel.efi"),
    ("initrd", "EFI/nixos/initrd.efi"),
    ("loader-config", "loader/loader.conf"),
    ("entry", ENTRY),
    ("boot-bin-record", "sekka/boot-bin.json"),
]


def canonical(value) -> bytes:
    def check(v):
        if isinstance(v, float):
            raise ValueError("manifests contain integers only")
        for item in v.values() if isinstance(v, dict) else v if isinstance(v, list) else []:
            check(item)

    check(value)
    return json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode()


def digest_of(data: bytes) -> str:
    return "sha256:" + hashlib.sha256(data).hexdigest()


def digest_file(path, f=None) -> tuple:
    h, size = hashlib.sha256(), 0
    with f or open(path, "rb") as stream:
        while chunk := stream.read(4 * MIB):
            h.update(chunk)
            size += len(chunk)
    return "sha256:" + h.hexdigest(), size


def align(n: int) -> int:
    return -(-n // MIB) * MIB


def inventory(esp: Path) -> list:
    files = []
    for dirpath, dirnames, filenames in os.walk(esp):
        dirnames.sort()
        for name in filenames:
            path = Path(dirpath) / name
            d, size = digest_file(path)
            files.append({"path": path.relative_to(esp).as_posix(), "size": size, "digest": d})
    return sorted(files, key=lambda f: f["path"].encode())


def fail(message: str):
    print(f"manifest: {message}", file=sys.stderr)
    sys.exit(1)


def main() -> int:
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    for arg in [
        "release", "profile", "variant", "revision", "clean", "nar-hash", "update-ref", "lock",
        "toplevel", "closure-info", "seeds", "root-image", "root-label", "root-uuid", "esp",
        "esp-bytes", "esp-volume-id", "generations", "payload", "installer-data", "stub-bytes", "out",
    ]:
        p.add_argument(f"--{arg}", required=True)
    a = p.parse_args()
    esp, root_image = Path(a.esp), Path(a.root_image)
    esp_bytes, stub = int(a.esp_bytes), int(a.stub_bytes)

    files = inventory(esp)
    by_path = {f["path"]: f for f in files}
    for role, path in BOOT_FILES:
        if path not in by_path:
            fail(f"the ESP has no {path} ({role})")
    entry = (esp / ENTRY).read_text()
    placeholders = [ph for ph in PLACEHOLDERS if ph in entry]
    for ph in placeholders:
        if entry.count(ph) != 1:
            fail(f"{ENTRY} has {ph} {entry.count(ph)} times")
    if a.variant == "product" and placeholders != PLACEHOLDERS:
        fail(f"the product image's {ENTRY} must carry {PLACEHOLDERS}")

    # Room on the ESP: every retained generation's kernel and initrd, the
    # firmware archive, boot.bin's previous copy and a replacement in flight.
    generations = int(a.generations)
    generation_bytes = by_path["EFI/nixos/kernel.efi"]["size"] + by_path["EFI/nixos/initrd.efi"]["size"]
    used = sum(f["size"] for f in files)
    boot_bin = by_path["m1n1/boot.bin"]["size"]
    needed = used + (generations - 1) * generation_bytes + 2 * boot_bin + FIRMWARE_ALLOWANCE
    # FAT's own tables and cluster rounding: keep a twentieth spare.
    if needed > esp_bytes * 19 // 20:
        fail(f"the {esp_bytes}-byte ESP can't hold {generations} generations of {generation_bytes} bytes: {needed} needed")

    root_digest, root_size = digest_file(root_image)
    if root_size % 4096:
        fail(f"root.img is {root_size} bytes, not a multiple of 4096")

    # The payload is exactly the ESP tree and the root image.
    payload = Path(a.payload)
    with zipfile.ZipFile(payload) as z:
        members = {i.filename: i for i in z.infolist() if not i.is_dir()}
        expected = {"esp/" + f["path"] for f in files} | {"root.img"}
        if set(members) != expected:
            fail(f"the payload holds {sorted(set(members) ^ expected)[:10]} beyond or instead of its inventory")
        for name, info in members.items():
            if info.compress_type != zipfile.ZIP_STORED:
                fail(f"{name} is compressed in the payload")
            got = digest_file(None, z.open(info))
            want = (root_digest, root_size) if name == "root.img" else (by_path[name[4:]]["digest"], by_path[name[4:]]["size"])
            if got != want:
                fail(f"{name} in the payload is {got}, not {want}")
    payload_digest, payload_size = digest_file(payload)

    data_path = Path(a.installer_data)
    data = json.loads(data_path.read_bytes())
    os_entry = data["os_list"][0]
    parts = {p["name"]: p for p in os_entry["partitions"]}
    if os_entry["package"] != payload.name or parts["EFI"]["size"] != f"{esp_bytes}B" or parts["Root"]["size"] != f"{root_size}B":
        fail(f"installer_data.json doesn't describe this payload: {os_entry}")
    if parts["EFI"]["volume_id"] != a.esp_volume_id:
        fail("installer_data.json gives the ESP another volume ID")
    data_digest, data_size = digest_file(data_path)

    closure_paths = len((Path(a.closure_info) / "store-paths").read_text().split())
    closure_bytes = int((Path(a.closure_info) / "total-nar-size").read_text())
    bootspec = json.loads((Path(a.toplevel) / "boot.json").read_text())["org.nixos.bootspec.v1"]

    reserve = max(root_size, UPDATE_FLOOR)
    minimum = align(stub + esp_bytes + root_size + reserve)
    recommended = align(minimum + reserve + DATA_ROOM)

    manifest = {
        "schema": SCHEMA,
        "release": a.release,
        "profile": a.profile,
        "variant": a.variant,
        "source": {
            "repository": "bitemyapp/sekka",
            "revision": a.revision,
            "clean": a.clean == "true",
            "nar_hash": a.nar_hash,
            "update_ref": a.update_ref,
            "lock": digest_file(a.lock)[0],
            "lock_bytes": digest_file(a.lock)[1],
        },
        "system": {
            "toplevel": a.toplevel,
            "label": bootspec["label"],
            "kernel_params": bootspec["kernelParams"],
            "closure_paths": closure_paths,
            "closure_nar_bytes": closure_bytes,
            "seeded_sources": sorted(Path(a.seeds).read_text().split()),
        },
        "image": {
            "root_image": {"size": root_size, "digest": root_digest},
            "esp_inventory": digest_of(canonical(files)),
            "esp_bytes": esp_bytes,
        },
        "root": {
            "label": a.root_label,
            "filesystem_uuid": a.root_uuid,
            "registered_paths": closure_paths,
        },
        "esp": {
            "volume_id": a.esp_volume_id,
            "files": files,
            "used_bytes": used,
            "boot_files": [{"role": r, "path": p} for r, p in BOOT_FILES],
            "boot_entry": ENTRY,
            "placeholders": placeholders,
            "generation_bytes": generation_bytes,
            "generations": generations,
        },
        "payload": {"name": payload.name, "size": payload_size, "digest": payload_digest},
        "installer_data": {"size": data_size, "digest": data_digest},
        "allocation": {"minimum": minimum, "recommended": recommended},
        "allocation_basis": {
            "stub": stub,
            "esp": esp_bytes,
            "root_image": root_size,
            "generation_reserve": reserve,
            "data_room": DATA_ROOM,
        },
        "handoff": {
            "schema": "sekka.mac.handoff/1",
            "path": "sekka/install.json",
            "kernel_arguments": ["sekka.root", "sekka.esp"],
        }
        if placeholders
        else None,
    }
    # The catalog's own consistency rules (catalog.rs, Release::check).
    if not (0 < payload_size <= 64 * GIB and data_size <= MIB and root_size <= payload_size):
        fail("the artifact sizes break the catalog's limits")
    if not (minimum >= root_size + esp_bytes and recommended >= minimum):
        fail("the allocation breaks the catalog's rules")
    out = canonical(manifest)
    if len(out) > 4 * MIB:
        fail("the manifest is larger than the catalog accepts")
    Path(a.out).write_bytes(out)
    print(f"manifest: {a.release}: root.img {root_size} bytes {root_digest}; payload {payload_size} bytes; "
          f"minimum {minimum}, recommended {recommended}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
