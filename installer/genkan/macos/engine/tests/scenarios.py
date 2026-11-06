# SPDX-License-Identifier: GPL-3.0-or-later
"""Whole runs of the engine against the fake Mac, shared by the tests and by
make_vectors.py. Each returns the transcript lines and what the run left."""

import support as S


def clean_inspection(t, privileged=True, builder=None):
    mac = (builder or S.j514s_clean)(t.sub("mac"), privileged=privileged)
    asahi, host = S.load_asahi(mac)
    outcome, doc, channel = S.inspect(mac, asahi, host, t.sub("work-inspect"), privileged=privileged)
    return mac, asahi, host, outcome, doc, channel


def execute(t, builder=S.j514s_clean, sekka=190 * S.GIB, place="shrink", before=None, plan_edit=None, release_kw=None, run_kw=None):
    """Inspect, derive a plan, optionally change something, and execute."""
    mac = builder(t.sub("mac"))
    asahi, host = S.load_asahi(mac)
    outcome, doc, _ = S.inspect(mac, asahi, host, t.sub("work-inspect"))
    assert outcome == "inspected", outcome
    release = S.make_release(t.sub("assets"), **(release_kw or {}))
    plan = S.derive_plan(doc, release, sekka, place=place)
    if plan_edit:
        plan_edit(plan)
    if before:
        before(mac, host, release)
    state = mac.state_digest()
    outcome, channel = S.run_engine("execute", mac, asahi, host, t.sub("work"), plan=plan, release=release, **(run_kw or {}))
    return {
        "mac": mac,
        "host": host,
        "asahi": asahi,
        "inspection": doc,
        "plan": plan,
        "release": release,
        "outcome": outcome,
        "channel": channel,
        "state_before": state,
    }


RESIZE_69606 = (
    1,
    b"Started APFS operation on disk3\n"
    b"Aligning shrink delta to 203,990,056,960 bytes and targeting a new physical store size of 290,373,738,496 bytes\n"
    b"Determined the minimum size for the targeted physical store of this APFS Container to be 188,000,239,616 bytes\n"
    b"Resizing APFS Container designated by APFS Container Reference disk3\n"
    b"The specific APFS Physical Store being resized is disk0s2\n"
    b"Verifying storage system\n"
    b"Error: -69606: A problem occurred while resizing APFS Container structures\n",
)


def resize_fails(mac, host, release):
    mac.resize_failure = RESIZE_69606


def layout_changes(mac, host, release):
    """macOS's container moves between planning and executing."""
    p = mac.part_by_name("disk0s2")
    p.size -= S.GIB
    p.container.ceiling -= S.GIB
    mac.limits[p.uuid]["CurrentSize"] = p.size


def read_back_corrupts(mac, host, release):
    host.corrupt_read_back = True


def bless_fails(mac, host, release):
    mac.bless_mode = "fail"


def bless_refuses_the_password(mac, host, release):
    """bless exits 0 but changes nothing: the Mac's owner has another
    password than the one the engine was given."""
    mac.owner_password = b"another password"


def retry(r, t, name="work-retry", bless_mode="ok", owner_password=S.PASSWORD, **kw):
    """The helper's bounded retry on the Mac an execute run left: the
    startup-disk mode with the same plan and installation."""
    mac = r["mac"]
    mac.bless_mode = bless_mode
    mac.owner_password = owner_password
    start = len(mac.commands)
    mounts = len(mac.mount_log)
    state = mac.state_digest()
    plan = kw.pop("plan", r["plan"])
    kw.setdefault("esp_inventory", esp_inventory(r.get("channel")))
    outcome, channel = S.run_engine("startup-disk", mac, r["asahi"], r["host"], t.sub(name), plan=plan, **kw)
    return {
        "mac": mac,
        "plan": plan,
        "outcome": outcome,
        "channel": channel,
        "commands": mac.commands[start:],
        "mounts": mac.mount_log[mounts:],
        "state_before": state,
    }


def esp_inventory(channel):
    """The esp-inventory fact of an execute run's Esp checkpoint, as the
    helper's journal keeps it."""
    for m in channel.messages() if channel is not None else []:
        payload = m["message"]["payload"]
        if m["message"]["type"] == "checkpoint" and payload["stage"] == "esp":
            return {f["name"]: f["value"] for f in payload["facts"]}["esp-inventory"]
    return S.canonical.digest_bytes(b"no esp checkpoint")


def find_plan_digest(channel):
    for m in channel.messages() if channel is not None else []:
        if m["message"]["type"] == "intent":
            return m["message"]["payload"]["plan"]
    return None


def write_lines(path, lines):
    with open(path, "wb") as f:
        for line in lines:
            f.write(line + b"\n")


def read_lines(path):
    with open(path, "rb") as f:
        return [line for line in f.read().split(b"\n") if line]
