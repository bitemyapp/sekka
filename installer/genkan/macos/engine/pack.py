# SPDX-License-Identifier: GPL-3.0-or-later
"""Pack the engine archive deterministically (default.nix runs this).

    python3 -I pack.py ROOT ASAHI_TARBALL OUT

ROOT is the extracted Asahi installer with sekka_engine/ and
sekka-profiles/ added. This writes ROOT/engine-lock.json, then
OUT/sekka-engine-0.9.2-sekka.1.tar.gz (names sorted, owner 0:0, modes 0755
or 0644, time 0, gzip without a name or time) and OUT/engine.json
describing it.
"""

import gzip
import hashlib
import io
import json
import os
import stat
import sys
import tarfile

ASAHI = {
    "version": "0.9.2",
    "tag": "v0.9.2",
    "commit": "dffbb38ef0c00c0431c609ecd8a00f42deb5b24c",
    "tarball": {
        "url": "https://cdn.asahilinux.org/installer/installer-v0.9.2.tar.gz",
        "sha256": "484d999e53b4aef27d17c4478e49567e96842f8a6cb35c073a0df5bf0bbe8276",
    },
}
RUNTIME = "Frameworks/Python.framework/Versions/3.13/bin/python3.13"
ENTRY = "sekka_engine/__main__.py"


def sha256_file(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for block in iter(lambda: f.read(1 << 20), b""):
            h.update(block)
    return h.hexdigest()


def write_lock(root, tarball):
    sys.dont_write_bytecode = True
    sys.path.append(root)
    from sekka_engine import ENGINE_VERSION, canonical, contract, provenance

    if sha256_file(tarball) != ASAHI["tarball"]["sha256"]:
        raise SystemExit("the Asahi tarball isn't the pinned one")
    with open(os.path.join(root, "version.tag")) as f:
        if f.read().strip() != ASAHI["tag"]:
            raise SystemExit("the extracted installer isn't v0.9.2")
    for path in (RUNTIME, ENTRY):
        if not os.path.isfile(os.path.join(root, path)):
            raise SystemExit(f"{path} is missing")
    profiles = sorted(os.listdir(os.path.join(root, provenance.PROFILE_DIR)))
    if profiles != ["j514s.json"]:
        raise SystemExit(f"unexpected profiles {profiles}")
    with open(os.path.join(root, provenance.PROFILE_DIR, "j514s.json"), "rb") as f:
        profile, profile_digest = contract.load_profile(f.read())
    package = os.path.join(root, "sekka_engine")
    lock = {
        "schema": provenance.LOCK_SCHEMA,
        "engine": ENGINE_VERSION,
        "asahi": ASAHI,
        "adapter": {
            "digest": provenance.adapter_digest(package),
            "files": provenance.adapter_files(package),
        },
        "profile": {"file": "j514s.json", "id": profile["id"], "digest": profile_digest},
        "runtime": RUNTIME,
        "entry": ENTRY,
    }
    data = canonical.dumps(lock)
    with open(os.path.join(root, provenance.LOCK_FILE), "wb") as f:
        f.write(data)
    return lock


def entries(root):
    if os.path.exists(os.path.join(root, "sekka_engine", "__pycache__")):
        raise SystemExit("bytecode was written into the adapter; run with -B")
    """Every path under root, sorted by its bytes (so parents come first).
    Nothing of Asahi's is left out: its runtime's precompiled modules are
    part of what its code signature seals."""
    out = []
    for dirpath, dirnames, filenames in os.walk(root):
        for name in dirnames + filenames:
            out.append(os.path.relpath(os.path.join(dirpath, name), root))
    return sorted(out, key=lambda p: p.encode("utf-8"))


def pack(root, out_path):
    buf = io.BytesIO()
    with tarfile.open(fileobj=buf, mode="w", format=tarfile.GNU_FORMAT) as tar:
        for rel in entries(root):
            path = os.path.join(root, rel)
            st = os.lstat(path)
            info = tarfile.TarInfo(rel)
            info.uid = info.gid = 0
            info.uname = info.gname = ""
            info.mtime = 0
            if stat.S_ISLNK(st.st_mode):
                info.type = tarfile.SYMTYPE
                info.linkname = os.readlink(path)
                info.mode = 0o777
                tar.addfile(info)
            elif stat.S_ISDIR(st.st_mode):
                info.type = tarfile.DIRTYPE
                info.mode = 0o755
                tar.addfile(info)
            elif stat.S_ISREG(st.st_mode):
                info.type = tarfile.REGTYPE
                info.mode = 0o755 if st.st_mode & 0o111 else 0o644
                info.size = st.st_size
                with open(path, "rb") as f:
                    tar.addfile(info, f)
            else:
                raise SystemExit(f"{rel} is neither a file, a folder nor a link")
    with open(out_path, "wb") as raw:
        with gzip.GzipFile(filename="", mode="wb", fileobj=raw, mtime=0, compresslevel=9) as gz:
            gz.write(buf.getvalue())


def main(root, tarball, out):
    lock = write_lock(root, tarball)
    os.makedirs(out, exist_ok=True)
    name = "sekka-engine-0.9.2-sekka.1.tar.gz"
    path = os.path.join(out, name)
    pack(root, path)
    from sekka_engine import canonical

    digest = sha256_file(path)
    meta = {
        "schema": "sekka.mac.engine-artifact/1",
        "file": name,
        "size": os.path.getsize(path),
        "sha256": digest,
        "digest": "sha256:" + digest,
        "engine": lock["engine"],
        "asahi": lock["asahi"],
        "adapter": lock["adapter"]["digest"],
        "profile": lock["profile"],
        "runtime": lock["runtime"],
        "entry": lock["entry"],
    }
    with open(os.path.join(out, "engine.json"), "wb") as f:
        f.write(canonical.dumps(meta) + b"\n")
    with open(os.path.join(out, "engine-lock.json"), "wb") as f:
        f.write(canonical.dumps(lock) + b"\n")
    json.dump(meta, sys.stdout, indent=2)
    print()


if __name__ == "__main__":
    if len(sys.argv) != 4:
        raise SystemExit(__doc__)
    main(*sys.argv[1:])
