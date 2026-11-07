"""manifest.py against the readers its manifest has (tests.nix): for a small
release made here, the engine's own sekka_engine.assets (load_manifest,
check_payload, load_template, esp_inventory_digest) must accept what
manifest.py writes, and the catalog's objects must have catalog.rs's shapes.

  python3 test_manifest.py MANIFEST_PY ENGINE_DIR IDENTITIES WORK
"""

import json
import subprocess
import sys
import zipfile
from pathlib import Path

manifest_py, engine, identities, work = (Path(p) for p in sys.argv[1:5])
sys.path.insert(0, str(engine))
from sekka_engine import assets, canonical  # noqa: E402

MIB = 1024 * 1024
GIB = 1024 * MIB
STUB = 2_500_000_000 // MIB * MIB
REVISION = "4882dc33652612e7475f467f6373d8b607e5407c"
ENTRY = "loader/entries/nixos-generation-1.conf"

esp = work / "esp"
files = {
    "m1n1/boot.bin": b"m1n1 stage 2 " * 1000,
    "EFI/BOOT/BOOTAA64.EFI": b"systemd-boot" * 100,
    "EFI/systemd/systemd-bootaa64.efi": b"systemd-boot" * 100,
    "EFI/nixos/kernel.efi": b"kernel" * 30000,
    "EFI/nixos/initrd.efi": b"initrd" * 20000,
    "loader/loader.conf": b"timeout 3\ndefault nixos-generation-1.conf\n",
    ENTRY: b"title Sekka\nlinux /EFI/nixos/kernel.efi\ninitrd /vendorfw/firmware.cpio\ninitrd /EFI/nixos/initrd.efi\n"
    b"options init=/nix/store/x/init sekka.root=PARTUUID=@SEKKA_ROOT_PARTUUID@ sekka.esp=PARTUUID=@SEKKA_ESP_PARTUUID@\n",
    "sekka/boot-bin.json": b'{"schema":"sekka.mac.boot-bin/1"}',
}
for path, data in files.items():
    (esp / path).parent.mkdir(parents=True, exist_ok=True)
    (esp / path).write_bytes(data)
root = work / "root.img"
root.write_bytes(b"\x53\xef" * (3 * 2048))
name = "sekka-j514s-test"
payload = work / f"{name}.zip"
with zipfile.ZipFile(payload, "w", zipfile.ZIP_STORED) as z:
    for path in sorted(files):
        z.write(esp / path, "esp/" + path)
    z.write(root, "root.img")
data = work / "installer_data.json"
data.write_text(json.dumps({"os_list": [{
    "name": "Sekka (test)", "default_os_name": "Sekka", "boot_object": "m1n1.bin", "next_object": "m1n1/boot.bin",
    "package": payload.name, "supported_fw": ["14.8.3"],
    "partitions": [
        {"name": "EFI", "type": "EFI", "size": f"{GIB}B", "format": "fat", "copy_firmware": True,
         "copy_installer_data": True, "source": "esp"},
        {"name": "Root", "type": "Linux", "size": f"{root.stat().st_size}B", "expand": True, "image": "root.img"},
    ]}]}))
closure = work / "closure-info"
closure.mkdir()
(closure / "store-paths").write_text("/nix/store/a-system\n/nix/store/b-source\n")
(closure / "total-nar-size").write_text("123456\n")
toplevel = work / "toplevel"
toplevel.mkdir()
(toplevel / "boot.json").write_text(json.dumps({"org.nixos.bootspec.v1": {"label": "test", "kernelParams": ["quiet"]}}))
(work / "flake.lock").write_text("{}\n")
(work / "seeds").write_text("/nix/store/b-source\n")

out = work / "manifest.json"
subprocess.run([sys.executable, str(manifest_py),
                "--release", name, "--profile", "j514s", "--variant", "product", "--revision", REVISION,
                "--clean", "true", "--nar-hash", "sha256-x", "--update-ref", "fix/apple-silicon-aizome",
                "--lock", str(work / "flake.lock"), "--toplevel", str(toplevel), "--closure-info", str(closure),
                "--seeds", str(work / "seeds"), "--root-image", str(root), "--identities", str(identities),
                "--esp", str(esp), "--esp-bytes", str(GIB), "--generations", "5", "--payload", str(payload),
                "--installer-data", str(data), "--stub-bytes", str(STUB), "--out", str(out)], check=True)
raw = out.read_bytes()
m = json.loads(raw)
assert canonical.dumps(m) == raw, "the manifest isn't in the contract's canonical form"

# A plan for this release at its minimum allocation, as plan.rs derives it.
plan = {
    "release": {"release": name, "source_revision": REVISION, "root_image": m["image"]["root_image"]["digest"]},
    "allocation": {"esp": GIB, "root": m["allocation"]["minimum"] - STUB - GIB},
}
doc, esp_files = assets.load_manifest(str(out), plan)
zf = assets.check_payload(str(payload), doc, esp_files)
assets.close_zip(zf)


class AsahiUtil:
    """psize and align_up as Asahi's installer's util.py has them."""

    @staticmethod
    def psize(v):
        v = v.upper().replace(" ", "")
        base = 1000
        if v[-2] == "I":
            base, v = 1024, v[:-2] + v[-1]
        for suffix, power in {"TB": 4, "GB": 3, "MB": 2, "KB": 1, "B": 0, "": 0}.items():
            if v.endswith(suffix):
                return int(float(v[: -len(suffix)]) * (base**power))

    @staticmethod
    def align_up(v, a=16384):
        return (v + a - 1) & ~(a - 1)


assets.load_template(str(data), AsahiUtil, plan, "14.8.3")
assert assets.esp_inventory_digest(m["esp"]["files"]) == m["image"]["esp_inventory"], "esp_inventory isn't the engine's form"

# The catalog's Release takes these as they are (catalog.rs, deny_unknown_fields).
shapes = {
    ("source",): {"repository", "revision", "lock"},
    ("image",): {"root_image", "esp_inventory", "esp_bytes"},
    ("image", "root_image"): {"size", "digest"},
    ("allocation",): {"minimum", "recommended"},
}
for path, keys in shapes.items():
    value = m
    for k in path:
        value = value[k]
    assert set(value) == keys, f"{'.'.join(path)} has {sorted(value)}, not catalog.rs's {sorted(keys)}"
assert m["source"]["revision"] == m["source_revision"] == REVISION
assert m["esp_layout"]["volume_id"] is None

# The failing control: the engine's reader refuses an esp object with more
# than its two keys, as manifest.py once wrote it.
old = dict(m, esp=dict(m["esp"], volume_id="0x5ecca5e1"))
(work / "old.json").write_bytes(canonical.dumps(old))
try:
    assets.load_manifest(str(work / "old.json"), plan)
except assets.AssetError as e:
    assert "unexpected fields" in str(e), e
else:
    raise AssertionError("the engine accepted an esp object with extra keys")
print(f"manifest: the engine reads {name}'s manifest, payload and installer data; esp_inventory {m['image']['esp_inventory']}")
