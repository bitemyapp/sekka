"""This installation's identity after the initrd: the first start's record
in /etc/nixos/identity.nix, and the boot entries NixOS rewrites.

  install_identity.py record --esp /boot --nixos /etc/nixos
      At the first start (sekka-install-identity.service): read the handoff
      the installer left on the ESP (sekka/install.json, sekka.mac.handoff/1),
      check it against the kernel command line and the partitions actually
      mounted at / and the ESP, and write identity.nix once. If identity.nix
      already exists, check that it names the same partitions instead.

  install_identity.py fill-entries --esp /boot [--root UUID --esp-partuuid UUID]
      After NixOS's systemd-boot installer (extraInstallCommands): put this
      installation's partition UUIDs in place of the image's placeholders in
      every boot entry. NixOS rewrites the first generation's entry from its
      kernel parameters, which carry the placeholders the installer filled.
      The UUIDs come from identity.nix's configuration when given, else from
      the running system, whose initrd checked them.

Nothing is written when any check fails. Every value here is a GPT partition
UUID, lowercase, as the installer (installer/genkan/macos, handoff.rs) and
udev write them; labels and filesystem UUIDs are never used, as every Sekka
image shares them.
"""

import argparse
import json
import os
import re
import subprocess
import sys
import tempfile
from pathlib import Path

SCHEMA = "sekka.mac.handoff/1"
HANDOFF_PATH = "sekka/install.json"
HANDOFF_LIMIT = 16 * 1024
PLACEHOLDERS = {"root": "@SEKKA_ROOT_PARTUUID@", "esp": "@SEKKA_ESP_PARTUUID@"}
UUID = re.compile(r"[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}")
REVISION = re.compile(r"[0-9a-f]{40}")
SLUG = re.compile(r"[a-z0-9][a-z0-9._-]{0,95}")
TIMESTAMP = re.compile(r"\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z")
HANDOFF_FIELDS = {"schema", "installation", "release", "source_revision", "root", "esp", "created", "hints"}
HINT_FIELDS = {"locale", "keyboard", "timezone", "computer_name", "full_name"}


class Refused(Exception):
    """A check failed; nothing was written."""


def canonical(value) -> bytes:
    """The contract's canonical JSON (installer/genkan/macos, canonical.rs):
    keys in byte order, no whitespace, integers only."""

    def check(v, depth=0):
        if depth > 32:
            raise Refused("the handoff nests too deeply")
        if isinstance(v, float):
            raise Refused("the handoff contains a fraction")
        if isinstance(v, dict):
            for item in v.values():
                check(item, depth + 1)
        if isinstance(v, list):
            for item in v:
                check(item, depth + 1)

    check(value)
    return json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode()


def parse_handoff(data: bytes) -> dict:
    """sekka.mac.handoff/1, as Handoff::parse reads it."""
    if len(data) > HANDOFF_LIMIT:
        raise Refused(f"{HANDOFF_PATH} is {len(data)} bytes; at most {HANDOFF_LIMIT} are accepted")
    try:
        value = json.loads(data)
    except ValueError as e:
        raise Refused(f"{HANDOFF_PATH} isn't JSON: {e}") from None
    if not isinstance(value, dict) or value.get("schema") != SCHEMA:
        found = value.get("schema") if isinstance(value, dict) else None
        raise Refused(f"{HANDOFF_PATH} is not a {SCHEMA} document (schema {found!r})")
    if canonical(value) != data:
        raise Refused(f"{HANDOFF_PATH} isn't in canonical form")
    if set(value) != HANDOFF_FIELDS:
        raise Refused(f"{HANDOFF_PATH} has fields {sorted(value)}, expected {sorted(HANDOFF_FIELDS)}")
    for field, pattern in [
        ("installation", UUID),
        ("root", UUID),
        ("esp", UUID),
        ("release", SLUG),
        ("source_revision", REVISION),
        ("created", TIMESTAMP),
    ]:
        if not isinstance(value[field], str) or not pattern.fullmatch(value[field]):
            raise Refused(f"{HANDOFF_PATH}: {field} is {value[field]!r}")
    hints = value["hints"]
    if not isinstance(hints, dict) or not set(hints) <= HINT_FIELDS:
        raise Refused(f"{HANDOFF_PATH}: unexpected hints {hints!r}")
    for k, v in hints.items():
        if v is not None and not isinstance(v, str):
            raise Refused(f"{HANDOFF_PATH}: hint {k} is {v!r}")
    if value["root"] == value["esp"]:
        raise Refused(f"{HANDOFF_PATH}: the root and the ESP are the same partition")
    return value


def from_cmdline(text: str) -> dict:
    """The partition UUIDs on the kernel command line, which the initrd checked."""
    found = {}
    for name in ("root", "esp"):
        values = [w.split("=", 1)[1] for w in text.split() if w.startswith(f"sekka.{name}=")]
        if len(values) != 1:
            raise Refused(f"the kernel command line has sekka.{name}= {len(values)} times")
        value = values[0]
        uuid = value.removeprefix("PARTUUID=")
        if uuid == value or not UUID.fullmatch(uuid):
            raise Refused(f"sekka.{name}={value} doesn't name a GPT partition UUID")
        found[name] = uuid
    if found["root"] == found["esp"]:
        raise Refused("sekka.root= and sekka.esp= name the same partition")
    return found


def identity_nix(root: str, esp: str, installation: str) -> str:
    return f"""# This installation's own partitions, written once at its first start by
# sekka-install-identity.service from the installer's handoff
# (/boot/{HANDOFF_PATH}, installation {installation}), after checking them
# against the kernel command line and the partitions mounted at / and /boot.
# They are GPT partition UUIDs, unique to this Mac's disk: every Sekka image
# has the same labels and filesystem UUIDs. Don't copy this file to another
# computer.
{{
  calamares.appleSilicon.identity = {{
    root = "{root}";
    esp = "{esp}";
  }};
}}
"""


def read_identity_nix(text: str) -> dict:
    found = {}
    for name in ("root", "esp"):
        m = re.findall(rf'^\s*{name}\s*=\s*"({UUID.pattern})";', text, re.M)
        if len(m) != 1:
            raise Refused(f"identity.nix doesn't name one {name} partition")
        found[name] = m[0]
    return found


def write_atomically(path: Path, data: bytes, mode: int | None = 0o644) -> None:
    """Write beside it, flush, then rename over it and flush the directory.
    The ESP's FAT has no permissions to set (mode None)."""
    fd, tmp = tempfile.mkstemp(dir=path.parent, prefix=f".{path.name}.", suffix=".tmp")
    try:
        with os.fdopen(fd, "wb") as f:
            f.write(data)
            f.flush()
            if mode is not None:
                os.fchmod(f.fileno(), mode)
            os.fsync(f.fileno())
        os.replace(tmp, path)
    except BaseException:
        Path(tmp).unlink(missing_ok=True)
        raise
    dirfd = os.open(path.parent, os.O_RDONLY | os.O_DIRECTORY)
    try:
        os.fsync(dirfd)
    finally:
        os.close(dirfd)


def record(handoff_bytes: bytes, cmdline: str, mounted: dict, nixos: Path) -> str:
    """Check everything; write identity.nix once. Returns what happened."""
    handoff = parse_handoff(handoff_bytes)
    running = from_cmdline(cmdline)
    for name in ("root", "esp"):
        if handoff[name] != running[name]:
            raise Refused(
                f"the handoff names {name} partition {handoff[name]}, "
                f"but this system was started with sekka.{name}=PARTUUID={running[name]}"
            )
        if mounted[name] != running[name]:
            raise Refused(
                f"the partition mounted as the {name} is {mounted[name] or 'not a GPT partition'}, "
                f"not {running[name]}"
            )
    target = nixos / "identity.nix"
    if target.exists():
        existing = read_identity_nix(target.read_text())
        if existing != {"root": running["root"], "esp": running["esp"]}:
            raise Refused(
                f"{target} names root {existing['root']} and ESP {existing['esp']}, "
                f"but this system is installation {handoff['installation']} on root {running['root']} "
                f"and ESP {running['esp']}; it was not changed"
            )
        return f"{target} already names this installation's partitions"
    write_atomically(target, identity_nix(running["root"], running["esp"], handoff["installation"]).encode())
    return f"wrote {target}: root {running['root']}, ESP {running['esp']} (installation {handoff['installation']})"


def fill(text: str, identity: dict) -> str:
    """Replace each placeholder; an entry has each once or not at all."""
    for name, placeholder in PLACEHOLDERS.items():
        n = text.count(placeholder)
        if n > 1:
            raise Refused(f"a boot entry has {placeholder} {n} times")
        text = text.replace(placeholder, identity[name])
    return text


def unfilled(esp: Path) -> list:
    """Boot entries that still carry a placeholder."""
    return [
        entry
        for entry in sorted((esp / "loader" / "entries").glob("*.conf"))
        if any(p in entry.read_text() for p in PLACEHOLDERS.values())
    ]


def fill_entries(esp: Path, identity: dict) -> list:
    changed = []
    for entry in unfilled(esp):
        write_atomically(entry, fill(entry.read_text(), identity).encode(), mode=None)
        changed.append(entry.name)
    return changed


def entry_identity(cmdline: str, configured: dict | None) -> dict:
    """Which UUIDs go into the entries. identity.nix's, when it exists, must
    be the running system's; a system started without sekka.* arguments (a
    rescue system in a chroot) uses identity.nix's alone."""
    has_arguments = any(w.startswith(("sekka.root=", "sekka.esp=")) for w in cmdline.split())
    if configured is None:
        if not has_arguments:
            raise Refused(
                "boot entries carry the image's placeholders, but neither identity.nix nor the "
                "kernel command line names this installation's partitions"
            )
        return from_cmdline(cmdline)
    if has_arguments:
        running = from_cmdline(cmdline)
        if configured != running:
            raise Refused(
                f"identity.nix names root {configured['root']} and ESP {configured['esp']}, "
                f"but this system runs from root {running['root']} and ESP {running['esp']}. "
                "Was it copied from another installation?"
            )
    return configured


def partuuid_of_mount(mountpoint: str) -> str | None:
    source = subprocess.run(
        ["findmnt", "-n", "-o", "SOURCE", "--target", mountpoint], capture_output=True, text=True, check=True
    ).stdout.strip()
    # The superblock-independent answer: the GPT entry itself (blkid -p).
    probe = subprocess.run(
        ["blkid", "-p", "-s", "PART_ENTRY_UUID", "-o", "value", source], capture_output=True, text=True
    )
    value = probe.stdout.strip().lower()
    return value or None


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    sub = parser.add_subparsers(dest="command", required=True)
    r = sub.add_parser("record")
    r.add_argument("--esp", type=Path, default=Path("/boot"))
    r.add_argument("--nixos", type=Path, default=Path("/etc/nixos"))
    f = sub.add_parser("fill-entries")
    f.add_argument("--esp", type=Path, default=Path("/boot"))
    f.add_argument("--root")
    f.add_argument("--esp-partuuid")
    args = parser.parse_args(argv)
    cmdline = Path("/proc/cmdline").read_text()
    try:
        if args.command == "record":
            handoff = args.esp / HANDOFF_PATH
            if not handoff.exists():
                raise Refused(f"{handoff} is missing: this system wasn't installed by Sekka's Mac installer")
            mounted = {"root": partuuid_of_mount("/"), "esp": partuuid_of_mount(str(args.esp))}
            print(record(handoff.read_bytes(), cmdline, mounted, args.nixos))
        else:
            configured = None
            if args.root or args.esp_partuuid:
                configured = {"root": args.root, "esp": args.esp_partuuid}
                if not all(v and UUID.fullmatch(v) for v in configured.values()):
                    raise Refused(f"identity.nix's partition UUIDs are malformed: {configured}")
            if unfilled(args.esp):
                changed = fill_entries(args.esp, entry_identity(cmdline, configured))
                print(f"sekka: this installation's partitions are now in {', '.join(changed)}")
    except Refused as e:
        print(f"sekka: {e}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
