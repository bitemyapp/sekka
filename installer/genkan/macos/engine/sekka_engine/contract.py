# SPDX-License-Identifier: GPL-3.0-or-later
"""The parts of sekka-mac-contract the engine speaks, in Python.

Each function here mirrors a Rust definition in
installer/genkan/macos/crates/contract/src/ and says which. Documents are
plain dicts serialized by `canonical.dumps`; the validators refuse anything
the Rust types would refuse, so the engine never writes a document the
helper can't read.
"""

import hashlib
import re
import unicodedata

from . import canonical

# lib.rs
GIB = 1024 * 1024 * 1024
GB = 1_000_000_000

LIMIT_PROFILE = 64 * 1024
LIMIT_INSPECTION = 512 * 1024
LIMIT_PLAN = 64 * 1024
LIMIT_ENGINE_LINE = 64 * 1024
LIMIT_RECOVERY = 64 * 1024
LIMIT_HANDOFF = 16 * 1024

ENGINE_SCHEMA = "sekka.mac.engine/1"
INSPECTION_SCHEMA = "sekka.mac.inspection/1"
PLAN_SCHEMA = "sekka.mac.plan/1"
PROFILE_SCHEMA = "sekka.mac.profile/1"
RECOVERY_SCHEMA = "sekka.mac.recovery/1"
HANDOFF_SCHEMA = "sekka.mac.handoff/1"

# plan.rs
ALIGN = 1024 * 1024
STUB_BYTES = 2_500_000_000 // ALIGN * ALIGN
ESP_BYTES = GIB
VOLUME_NAME = "Sekka"

# recovery.rs
RECOVERY_DATA_DIR = "Sekka/InstallData"
RECOVERY_CONFIG_FILE = "recovery.json"

# handoff.rs
HANDOFF_PATH = "sekka/install.json"
ROOT_ARGUMENT = "sekka.root"
ESP_ARGUMENT = "sekka.esp"
ROOT_PLACEHOLDER = "@SEKKA_ROOT_PARTUUID@"
ESP_PLACEHOLDER = "@SEKKA_ESP_PARTUUID@"

# inspection.rs
MACHINE_BINDING_DOMAIN = "sekka.mac.machine/1\n"

# profile.rs
KNOWN_BOARDS = (("Mac15,6", "j514sap", 0x6030),)

# engine.rs
ENGINE_PROTOCOL = 1

# record.rs: Stage::ORDER, in kebab-case.
STAGES = (
    "import",
    "apple-inputs",
    "shrink",
    "stub-partition",
    "stub-system",
    "partitions",
    "firmware",
    "esp",
    "root",
    "recovery",
    "startup-disk",
    "enrollment",
    "startup-selection",
)
RECOVERY_STAGES = ("enrollment", "startup-selection")
VERIFICATIONS = ("none", "inspected", "source-hashed", "flushed", "read-back", "policy-read-back")
DISK_CHANGED = ("no", "yes", "unknown")
LOG_LEVELS = ("debug", "info", "warning", "error")
OUTCOMES = ("inspected", "waiting-for-recovery", "failed")

PARTITION_KINDS = ("apple-isc", "apfs", "apple-recovery", "efi", "linux", "linux-swap", "other")
FILEVAULT = ("on", "off", "converting", "unknown")
INSTALLATION_KINDS = ("sekka", "other-linux", "unknown")
INSTALLATION_STATES = ("complete", "waiting-for-recovery", "partial", "unknown")
ISSUE_CODES = (
    "unsupported-machine",
    "untested-firmware",
    "macos-too-old",
    "known-macos-issue",
    "not-normal-boot",
    "on-battery",
    "battery-low",
    "file-vault-converting",
    "local-snapshots",
    "not-enough-space",
    "resize-limits-unavailable",
    "unrecognized-layout",
    "existing-sekka",
    "partial-installation",
    "ambiguous-disk",
)


class Invalid(ValueError):
    """A value or document the contract's types would refuse."""


def macos_stages(shrinks):
    """engine.rs `macos_stages`: the stages `execute` runs in macOS."""
    return [
        s
        for s in STAGES
        if s != "import" and s not in RECOVERY_STAGES and (shrinks or s != "shrink")
    ]


# ids.rs -------------------------------------------------------------------

_SLUG = re.compile(r"[a-z0-9][a-z0-9._-]{0,95}\Z")
_UUID = re.compile(r"[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}\Z")
_DIGEST = re.compile(r"sha256:[0-9a-f]{64}\Z")
_REVISION = re.compile(r"[0-9a-f]{40}\Z")
_TIMESTAMP = re.compile(r"(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2}):(\d{2})Z\Z")


def slug(value):
    if not isinstance(value, str) or not _SLUG.fullmatch(value) or not value.isascii():
        raise Invalid(f"not an identifier: {str(value)[:100]}")
    return value


def is_text(value):
    return (
        isinstance(value, str)
        and len(value.encode("utf-8", "surrogatepass")) <= 2048
        and not any(unicodedata.category(c) == "Cc" and c != "\n" for c in value)
        and not any(0xD800 <= ord(c) <= 0xDFFF for c in value)
    )


def text(value):
    """ids.rs Text: at most 2048 bytes, no control characters but `\\n`."""
    if not is_text(value):
        raise Invalid("text is at most 2 KiB with no control characters")
    return value


def plain_text(value, limit=2048, keep="end"):
    """Make any string acceptable as Text: line breaks become `\\n`, tabs
    spaces, other control characters are dropped, and it is cut to `limit`
    bytes (keeping its end, where tools print their error, unless asked)."""
    if isinstance(value, (bytes, bytearray)):
        value = bytes(value).decode("utf-8", "replace")
    value = value.replace("\r\n", "\n").replace("\r", "\n").replace("\t", " ")
    value = "".join(
        c
        for c in value
        if (unicodedata.category(c) != "Cc" or c == "\n") and not 0xD800 <= ord(c) <= 0xDFFF
    )
    value = value.strip()
    data = value.encode("utf-8")
    if len(data) <= limit:
        return value
    marker = "…"
    budget = limit - len(marker.encode("utf-8"))
    # Cutting the bytes may split a character; decoding drops the pieces.
    if keep == "end":
        return marker + data[-budget:].decode("utf-8", "ignore")
    return data[:budget].decode("utf-8", "ignore") + marker


def uuid_any(value):
    """ids.rs Uuid::parse_any: either case in, lower case out."""
    if not isinstance(value, str):
        raise Invalid("not a UUID")
    lowered = value.lower()
    if not _UUID.fullmatch(lowered) or not lowered.isascii():
        raise Invalid(f"not a UUID: {value[:40]}")
    return lowered


def uuid(value):
    """A UUID already in its lower-case document form."""
    if not isinstance(value, str) or not _UUID.fullmatch(value) or not value.isascii():
        raise Invalid(f"not a lowercase UUID: {str(value)[:40]}")
    return value


def digest(value):
    if not isinstance(value, str) or not _DIGEST.fullmatch(value) or not value.isascii():
        raise Invalid("digests are sha256: and 64 lowercase hex digits")
    return value


def revision(value):
    if not isinstance(value, str) or not _REVISION.fullmatch(value) or not value.isascii():
        raise Invalid("a revision is a full 40-digit commit")
    return value


def version_parts(value):
    """ids.rs Version: MAJOR.MINOR[.PATCH], no leading zeros."""
    if not isinstance(value, str):
        raise Invalid("not a version")
    pieces = value.split(".")
    if not 2 <= len(pieces) <= 3:
        raise Invalid(f"not a version: {value[:40]}")
    parts = []
    for p in pieces:
        if (
            not p
            or len(p) > 9
            or not p.isascii()
            or not p.isdigit()
            or (len(p) > 1 and p.startswith("0"))
        ):
            raise Invalid(f"not a version: {value[:40]}")
        parts.append(int(p))
    while len(parts) < 3:
        parts.append(0)
    return tuple(parts)


def version(value):
    version_parts(value)
    return value


def _days_in_month(y, m):
    if m == 2:
        return 29 if (y % 4 == 0 and y % 100 != 0) or y % 400 == 0 else 28
    return 30 if m in (4, 6, 9, 11) else 31


def timestamp(value):
    match = _TIMESTAMP.fullmatch(value) if isinstance(value, str) else None
    if not match:
        raise Invalid("not a UTC timestamp")
    y, mo, d, h, mi, s = (int(g) for g in match.groups())
    if not (1 <= mo <= 12 and 1 <= d <= _days_in_month(y, mo) and h <= 23 and mi <= 59 and s <= 59):
        raise Invalid("not a UTC timestamp")
    return value


def format_timestamp(unix_seconds):
    """ids.rs Timestamp's Display, from Unix seconds."""
    import datetime

    moment = datetime.datetime.fromtimestamp(int(unix_seconds), datetime.timezone.utc)
    return moment.strftime("%Y-%m-%dT%H:%M:%SZ")


def u64(value):
    if isinstance(value, bool) or not isinstance(value, int) or not 0 <= value <= canonical.U64_MAX:
        raise Invalid("expected an unsigned 64-bit integer")
    return value


def u32(value):
    if isinstance(value, bool) or not isinstance(value, int) or not 0 <= value <= 0xFFFFFFFF:
        raise Invalid("expected an unsigned 32-bit integer")
    return value


def fact(name, value):
    """record.rs Fact: a slug and a Text."""
    return {"name": slug(name), "value": text(str(value))}


# Strict shape checks for documents read from elsewhere ----------------------


def fields(obj, required, optional=()):
    """An object with exactly these keys (serde's deny_unknown_fields; an
    `Option` field may be missing, which serde reads as None)."""
    if not isinstance(obj, dict):
        raise Invalid("expected an object")
    keys = set(obj)
    missing = set(required) - keys
    unknown = keys - set(required) - set(optional)
    if missing or unknown:
        raise Invalid(f"unexpected fields {sorted(unknown)}, missing {sorted(missing)}")
    return obj


# inspection.rs --------------------------------------------------------------


def machine_binding(platform_uuid):
    """inspection.rs `machine_binding`."""
    lowered = uuid_any(platform_uuid)
    return canonical.digest_bytes((MACHINE_BINDING_DOMAIN + lowered).encode("ascii"))


def layout_digest(disk):
    """inspection.rs Disk::layout_digest: the disk, every partition in offset
    order, then every container's size and volume groups."""
    partitions = [[p["uuid"], p["kind"], p["offset"], p["size"]] for p in disk["partitions"]]
    partitions.sort(key=lambda p: p[2])
    containers = []
    for c in disk["containers"]:
        groups = sorted(g["uuid"] for g in c["volume_groups"])
        containers.append([c["uuid"], c["partition"], c["size"], groups])
    containers.sort(key=lambda c: (c[0], c[1], c[2], c[3]))
    return canonical.digest(
        {
            "domain": "sekka.mac.layout/1",
            "uuid": disk["uuid"],
            "size": disk["size"],
            "block_size": disk["block_size"],
            "partitions": partitions,
            "containers": containers,
        }
    )


def check_disk(disk):
    """inspection.rs Disk::check."""
    bs = disk["block_size"]
    if bs == 0 or bs & (bs - 1):
        raise Invalid("the disk's block size isn't a power of two")
    ordered = sorted(disk["partitions"], key=lambda p: p["offset"])
    for a, b in zip(ordered, ordered[1:]):
        if a["offset"] + a["size"] > b["offset"]:
            raise Invalid("partitions overlap")
    if ordered and ordered[-1]["offset"] + ordered[-1]["size"] > disk["size"]:
        raise Invalid("a partition extends past the disk")
    for c in disk["containers"]:
        if not any(p["uuid"] == c["partition"] and p["kind"] == "apfs" for p in disk["partitions"]):
            raise Invalid("a container names no APFS partition")


def check_inspection(doc):
    """inspection.rs Inspection::check, plus the field types serde checks."""
    disk = doc["disk"]
    for p in disk["partitions"]:
        uuid(p["uuid"])
        text(p["name"])
        if p["kind"] not in PARTITION_KINDS:
            raise Invalid("unknown partition kind")
        u64(p["offset"])
        u64(p["size"])
    for c in disk["containers"]:
        uuid(c["uuid"])
        uuid(c["partition"])
        for g in c["volume_groups"]:
            uuid(g["uuid"])
            text(g["name"])
            if g["os"]["kind"] == "macos":
                version(g["os"]["version"])
            elif g["os"]["kind"] == "stub":
                text(g["os"]["for_system"])
    uuid(disk["uuid"])
    text(disk["device"])
    m = doc["machine"]
    for key in ("product_type", "device_class", "product_name", "soc_name", "system_firmware"):
        text(m[key])
    u32(m["chip_id"])
    u32(m["board_id"])
    digest(m["binding"])
    version(doc["macos"]["version"])
    text(doc["macos"]["build"])
    version(doc["macos"]["system_firmware_version"])
    uuid(doc["macos"]["volume_group"])
    if doc["filevault"] not in FILEVAULT:
        raise Invalid("unknown FileVault state")
    for issue in doc["issues"]:
        if issue["code"] not in ISSUE_CODES:
            raise Invalid("unknown issue code")
        text(issue["detail"])
    for inst in doc["installations"]:
        if inst["kind"] not in INSTALLATION_KINDS or inst["state"] not in INSTALLATION_STATES:
            raise Invalid("unknown installation kind or state")
        text(inst["name"])
    check_disk(disk)
    if doc["layout_digest"] != layout_digest(disk):
        raise Invalid("the inspection's layout digest doesn't match its disk")
    for cand in doc["candidates"]:
        if cand["kind"] == "free-space":
            e = cand["extent"]
            end = e["offset"] + e["size"]
            if e["size"] == 0 or end > disk["size"]:
                raise Invalid("a free extent lies outside the disk")
            for p in disk["partitions"]:
                if p["offset"] < end and e["offset"] < p["offset"] + p["size"]:
                    raise Invalid("a free extent overlaps a partition")
        else:
            found = [
                k
                for k in disk["containers"]
                if k["uuid"] == cand["container"] and k["partition"] == cand["partition"]
            ]
            if not found:
                raise Invalid("a shrink candidate names no container")
            if (
                found[0]["size"] != cand["current_size"]
                or cand["available_recommended"] > cand["available"]
                or cand["available"] >= cand["current_size"]
            ):
                raise Invalid("a shrink candidate's sizes are inconsistent")


def candidate_capacity(candidate):
    """inspection.rs Candidate::capacity."""
    if candidate["kind"] == "free-space":
        return candidate["extent"]["size"]
    return candidate["available"] + candidate["adjacent_free"]


# profile.rs -----------------------------------------------------------------


def firmware_number(value):
    for prefix in ("iBoot-", "mBoot-"):
        if value.startswith(prefix):
            return value[len(prefix) :]
    return value


def check_profile(profile):
    """profile.rs HardwareProfile::check."""
    m = profile["machine"]
    if not any(
        p == m["product_type"] and d == m["device_class"] and c == m["chip_id"]
        for p, d, c in KNOWN_BOARDS
    ):
        raise Invalid(f"profile {profile['id']} names a board this engine doesn't know")
    if not profile["firmware"]["os_firmware"]:
        raise Invalid("a profile names the OS firmware its drivers need")
    if not profile["boot_stack"] or not profile["evidence"]:
        raise Invalid("a profile names its boot stack and evidence")
    if profile["support"] == "qualified" and any(
        e["status"] == "untested" for e in profile["evidence"]
    ):
        raise Invalid("a qualified profile can't rest on untested claims")
    return profile


def load_profile(data):
    """Read a profile as profile.rs `j514s()` does (relaxed bytes), and the
    digest plans bind it by (its canonical form's)."""
    profile = canonical.loads_relaxed(data, LIMIT_PROFILE, PROFILE_SCHEMA)
    check_profile(profile)
    for v in profile["firmware"]["os_firmware"]:
        version(v)
    version(profile["macos"]["minimum"])
    return profile, canonical.digest(profile)


def admit(profile, facts):
    """profile.rs HardwareProfile::admit: (refusals, acknowledgements), each
    a (kind, details) pair."""
    refusals, acknowledgements = [], []
    m = profile["machine"]
    if facts["product_type"] != m["product_type"] or facts["device_class"] != m["device_class"]:
        refusals.append(
            ("unknown-machine", {"product_type": facts["product_type"], "device_class": facts["device_class"]})
        )
    elif facts["chip_id"] != m["chip_id"]:
        refusals.append(("chip-mismatch", {"expected": m["chip_id"], "found": facts["chip_id"]}))
    found = firmware_number(facts["system_firmware"])
    tested = profile["firmware"]["system_firmware"]
    if found not in tested:
        refusal = ("untested-system-firmware", {"found": facts["system_firmware"], "tested": list(tested)})
        if profile["firmware"]["untested_system_firmware"] == "block":
            refusals.append(refusal)
        else:
            acknowledgements.append(refusal)
    if version_parts(facts["macos"]) < version_parts(profile["macos"]["minimum"]):
        refusals.append(("macos-too-old", {"minimum": profile["macos"]["minimum"], "found": facts["macos"]}))
    running = facts["macos"]
    for issue in profile["macos"]["known_issues"]:
        prefix = issue["macos"]
        if running == prefix or running.startswith(prefix + "."):
            refusal = ("known-issue", {"summary": issue["summary"], "reference": issue["reference"]})
            (refusals if issue["blocking"] else acknowledgements).append(refusal)
    return refusals, acknowledgements


# plan.rs --------------------------------------------------------------------

_ACTION_FIELDS = {
    "shrink-container": ("container", "from", "to"),
    "create-stub": ("size",),
    "create-esp": ("size",),
    "create-root": ("size",),
}
HUMAN_STEPS = ("shut-down", "hold-power-button", "choose-volume", "authorize-in-recovery", "create-account")
ACKNOWLEDGEMENTS = ("untested-system-firmware", "known-macos-issue", "development-release")


def parse_plan(data):
    """Read a plan file: canonical `sekka.mac.plan/1` bytes, every field as
    plan.rs declares it, and Plan::check's arithmetic. Returns the plan and
    its digest (the digest of exactly these bytes)."""
    plan = canonical.loads(data, LIMIT_PLAN, PLAN_SCHEMA)
    fields(
        plan,
        (
            "schema",
            "release",
            "machine",
            "disk",
            "allocation",
            "actions",
            "target",
            "volume_name",
            "human_steps",
            "acknowledgements",
        ),
    )
    r = fields(
        plan["release"],
        (
            "channel",
            "catalog_serial",
            "release",
            "profile",
            "profile_digest",
            "source_revision",
            "payload",
            "installer_data",
            "manifest",
            "engine",
            "recovery_app",
            "root_image",
        ),
    )
    slug(r["channel"])
    u64(r["catalog_serial"])
    slug(r["release"])
    slug(r["profile"])
    revision(r["source_revision"])
    for key in ("profile_digest", "payload", "installer_data", "manifest", "engine", "recovery_app", "root_image"):
        digest(r[key])
    m = fields(plan["machine"], ("product_type", "device_class", "chip_id", "system_firmware", "binding"))
    text(m["product_type"])
    text(m["device_class"])
    u32(m["chip_id"])
    text(m["system_firmware"])
    digest(m["binding"])
    d = fields(plan["disk"], ("disk", "layout_digest", "macos_volume_group"))
    uuid(d["disk"])
    digest(d["layout_digest"])
    uuid(d["macos_volume_group"])
    a = fields(plan["allocation"], ("sekka", "stub", "esp", "root"), ("macos_container_after",))
    for key in ("sekka", "stub", "esp", "root"):
        u64(a[key])
    a.setdefault("macos_container_after", None)
    if a["macos_container_after"] is not None:
        u64(a["macos_container_after"])
    if not isinstance(plan["actions"], list):
        raise Invalid("actions are a list")
    for action in plan["actions"]:
        kind = action.get("kind") if isinstance(action, dict) else None
        if kind not in _ACTION_FIELDS:
            raise Invalid("unknown action")
        fields(action, ("kind",) + _ACTION_FIELDS[kind])
        for key in _ACTION_FIELDS[kind]:
            if key == "container":
                uuid(action[key])
            else:
                u64(action[key])
    t = fields(plan["target"], ("offset", "size"))
    u64(t["offset"])
    u64(t["size"])
    text(plan["volume_name"])
    if not isinstance(plan["human_steps"], list) or any(s not in HUMAN_STEPS for s in plan["human_steps"]):
        raise Invalid("unknown human step")
    if not isinstance(plan["acknowledgements"], list) or any(
        s not in ACKNOWLEDGEMENTS for s in plan["acknowledgements"]
    ):
        raise Invalid("unknown acknowledgement")
    check_plan(plan)
    return plan, canonical.digest_bytes(bytes(data))


def check_plan(plan):
    """plan.rs Plan::check."""
    a = plan["allocation"]
    if a["stub"] + a["esp"] + a["root"] != a["sekka"] or a["sekka"] != plan["target"]["size"]:
        raise Invalid("the plan's allocation doesn't add up")
    if any(v % ALIGN for v in (plan["target"]["offset"], plan["target"]["size"], a["stub"], a["esp"], a["root"])):
        raise Invalid("the plan's partitions aren't aligned")
    creates = [x for x in plan["actions"] if x["kind"] != "shrink-container"]
    expected = [
        {"kind": "create-stub", "size": a["stub"]},
        {"kind": "create-esp", "size": a["esp"]},
        {"kind": "create-root", "size": a["root"]},
    ]
    if creates != expected:
        raise Invalid("the plan creates the stub, the ESP and the root, in order")
    first = plan["actions"][0] if plan["actions"] else None
    if first and first["kind"] == "shrink-container":
        if first["to"] >= first["from"] or a["macos_container_after"] != first["to"] or len(plan["actions"]) != 4:
            raise Invalid("the plan's shrink is inconsistent")
    elif a["macos_container_after"] is not None or len(plan["actions"]) != 3:
        raise Invalid("the plan's shrink is inconsistent")


def plan_shrink(plan):
    first = plan["actions"][0]
    return first if first["kind"] == "shrink-container" else None


# engine.rs ------------------------------------------------------------------


def engine_message(seq, kind, payload):
    return {"schema": ENGINE_SCHEMA, "seq": seq, "message": {"type": kind, "payload": payload}}


def failure(code, message, apple_diagnostic=None, disk_changed="unknown"):
    """record.rs Failure."""
    if disk_changed not in DISK_CHANGED:
        raise Invalid("unknown disk_changed")
    return {
        "code": slug(code),
        "message": text(plain_text(message, keep="start")),
        "apple_diagnostic": None if apple_diagnostic is None else text(plain_text(apple_diagnostic)),
        "disk_changed": disk_changed,
    }


# handoff.rs and recovery.rs -------------------------------------------------


def fill_entry(entry, root, esp):
    """handoff.rs Handoff::fill_entry."""
    for placeholder in (ROOT_PLACEHOLDER, ESP_PLACEHOLDER):
        if entry.count(placeholder) != 1:
            raise Invalid(f"the boot entry must contain {placeholder} once")
    filled = entry.replace(ROOT_PLACEHOLDER, uuid(root)).replace(ESP_PLACEHOLDER, uuid(esp))
    if not names_partitions(filled, root, esp):
        raise Invalid(
            f"the boot entry's options must pass {ROOT_ARGUMENT}=PARTUUID={ROOT_PLACEHOLDER} and "
            f"{ESP_ARGUMENT}=PARTUUID={ESP_PLACEHOLDER}"
        )
    return filled


def entry_arguments(entry):
    """What a boot entry passes the kernel: the words of its `options`
    lines, which systemd-boot joins into the command line."""
    words = []
    for line in entry.splitlines():
        parts = line.split()
        if parts and parts[0] == "options":
            words += parts[1:]
    return words


def names_partitions(entry, root, esp):
    """handoff.rs Handoff::names_partitions: the entry passes
    sekka.root=PARTUUID=ROOT and sekka.esp=PARTUUID=ESP, each an argument of
    its own, anywhere and in any order, as first-boot setup reads them from
    the command line."""
    words = entry_arguments(entry)

    def passes(argument, partition):
        prefix = f"{argument}=PARTUUID="
        return any(w.startswith(prefix) and w[len(prefix) :].lower() == partition.lower() for w in words)

    return passes(ROOT_ARGUMENT, root) and passes(ESP_ARGUMENT, esp)


def partition_arguments(entry):
    """The entry's arguments that name Sekka's partitions, in its order."""
    return " ".join(w for w in entry_arguments(entry) if w.startswith((ROOT_ARGUMENT + "=", ESP_ARGUMENT + "=")))


def kernel_arguments(root, esp):
    return f"{ROOT_ARGUMENT}=PARTUUID={root} {ESP_ARGUMENT}=PARTUUID={esp}"


def handoff(installation, release, source_revision, root, esp, created, hints):
    doc = {
        "schema": HANDOFF_SCHEMA,
        "installation": uuid(installation),
        "release": slug(release),
        "source_revision": revision(source_revision),
        "root": uuid(root),
        "esp": uuid(esp),
        "created": timestamp(created),
        "hints": {
            key: (None if hints.get(key) is None else text(hints[key]))
            for key in ("locale", "keyboard", "timezone", "computer_name", "full_name")
        },
    }
    if doc["root"] == doc["esp"]:
        raise Invalid("the root and the ESP are the same partition")
    data = canonical.dumps(doc)
    if len(data) > LIMIT_HANDOFF:
        raise Invalid("the handoff is too large")
    return doc


def parse_handoff(data):
    doc = canonical.loads(data, LIMIT_HANDOFF, HANDOFF_SCHEMA)
    fields(doc, ("schema", "installation", "release", "source_revision", "root", "esp", "created", "hints"))
    uuid(doc["installation"])
    uuid(doc["root"])
    uuid(doc["esp"])
    if doc["root"] == doc["esp"]:
        raise Invalid("the root and the ESP are the same partition")
    return doc


def inventory_file(path, data_or_size, file_digest=None):
    if file_digest is None:
        size, file_digest = len(data_or_size), canonical.digest_bytes(data_or_size)
    else:
        size = data_or_size
    return {"path": text(path), "size": u64(size), "digest": digest(file_digest)}


def check_inventory_file(item, limit):
    """recovery.rs InventoryFile::check."""
    p = item["path"]
    parts = p.split("/")
    plain = (
        p
        and not p.startswith("/")
        and all(c and c not in (".", "..") for c in parts)
        and len(p.encode("utf-8")) <= 255
    )
    if not plain:
        raise Invalid(f"not a plain relative path: {p}")
    if item["size"] == 0 or item["size"] > limit:
        raise Invalid(f"{p} is {item['size']} bytes")


def check_recovery_config(config):
    """recovery.rs RecoveryConfig::check."""
    check_inventory_file(config["boot_object"], 64 * 1024 * 1024)
    if not config["firmware"]:
        raise Invalid("the firmware baseline is empty")
    for item in config["firmware"]:
        check_inventory_file(item, 256 * 1024 * 1024)
    stub = config["stub"]
    # A group made with `-groupWith` takes its Data volume's UUID, so those
    # two may be equal; every other pair must differ.
    ids = [stub["container"], stub["system_volume"], stub["data_volume"], config["esp"], config["root"]]
    if len(set(ids)) != len(ids) or stub["volume_group"] in (
        stub["container"],
        stub["system_volume"],
        config["esp"],
        config["root"],
    ):
        raise Invalid("two of the target's identities are the same")
    if stub["volume_group"] == config["owner"]["macos_volume_group"]:
        raise Invalid("the stub can't be the owner's own macOS")


def parse_recovery_config(data):
    doc = canonical.loads(data, LIMIT_RECOVERY, RECOVERY_SCHEMA)
    fields(
        doc,
        (
            "schema",
            "installation",
            "release",
            "volume_name",
            "stub",
            "esp",
            "root",
            "boot_object",
            "firmware",
            "owner",
            "os_firmware",
        ),
    )
    fields(doc["stub"], ("container", "volume_group", "system_volume", "data_volume"))
    for key in ("installation", "esp", "root"):
        uuid(doc[key])
    check_recovery_config(doc)
    return doc


def sha256_file(path, chunk=8 * 1024 * 1024):
    h = hashlib.sha256()
    size = 0
    with open(path, "rb") as f:
        while True:
            block = f.read(chunk)
            if not block:
                break
            h.update(block)
            size += len(block)
    return size, "sha256:" + h.hexdigest()
