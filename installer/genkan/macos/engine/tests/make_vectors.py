# SPDX-License-Identifier: GPL-3.0-or-later
"""Write tests/vectors: documents and transcripts the engine produces against
the fake Mac. expected.json, the bytes and digests the Rust crate computes
for the documents, is written by crates/contract/examples/canonicalize; see
tests/vectors/README.md.

    SEKKA_ASAHI_ROOT=… python3 -I tests/make_vectors.py
"""

import json
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import scenarios  # noqa: E402
import support as S  # noqa: E402
from sekka_engine import canonical  # noqa: E402
from sekka_engine import contract as c  # noqa: E402


def pretty(path, doc):
    with open(path, "w") as f:
        json.dump(doc, f, indent=2, ensure_ascii=False, sort_keys=True)
        f.write("\n")


def main():
    out = S.VECTORS
    os.makedirs(out, exist_ok=True)
    transcripts = {}

    t = S.TempDir()
    try:
        mac, asahi, host, outcome, doc, channel = scenarios.clean_inspection(t)
        pretty(os.path.join(out, "inspection-j514s-clean.json"), doc)
        scenarios.write_lines(os.path.join(out, "inspect-j514s-clean.jsonl"), channel.lines)
        transcripts["inspect-j514s-clean.jsonl"] = {"mode": "inspect", "outcome": "Inspected"}
    finally:
        t.cleanup()

    for name, builder in (("existing-sekka", lambda root, **kw: S.j514s_existing_sekka(root, **kw)[0]), ("m1-pro-asahi", S.m1_pro_with_asahi)):
        t = S.TempDir()
        try:
            mac, asahi, host, outcome, doc, channel = scenarios.clean_inspection(t, builder=builder)
            pretty(os.path.join(out, f"inspection-{name}.json"), doc)
        finally:
            t.cleanup()

    t = S.TempDir()
    try:
        r = scenarios.execute(t)
        pretty(os.path.join(out, "plan-j514s-shrink.json"), r["plan"])
        scenarios.write_lines(os.path.join(out, "execute-happy.jsonl"), r["channel"].lines)
        transcripts["execute-happy.jsonl"] = {"mode": "execute", "plan": canonical.digest(r["plan"]), "shrinks": True, "outcome": "WaitingForRecovery"}
        mac = r["mac"]
        esp = [p for p in mac.parts if p.content == "EFI"][0]
        with open(os.path.join(esp.dir, c.HANDOFF_PATH), "rb") as f:
            pretty(os.path.join(out, "handoff.json"), json.loads(f.read()))
        for ct in mac.containers.values():
            for v in ct.volumes:
                path = os.path.join(v.dir, c.RECOVERY_DATA_DIR, c.RECOVERY_CONFIG_FILE)
                if os.path.exists(path):
                    with open(path, "rb") as f:
                        pretty(os.path.join(out, "recovery.json"), json.loads(f.read()))
    finally:
        t.cleanup()

    t = S.TempDir()
    try:
        r = scenarios.execute(t, builder=S.j514s_free_space, place="free-space", sekka=150 * S.GIB)
        pretty(os.path.join(out, "plan-j514s-free-space.json"), r["plan"])
        scenarios.write_lines(os.path.join(out, "execute-free-space.jsonl"), r["channel"].lines)
        transcripts["execute-free-space.jsonl"] = {"mode": "execute", "plan": canonical.digest(r["plan"]), "shrinks": False, "outcome": "WaitingForRecovery"}
    finally:
        t.cleanup()

    for name, before in (
        ("execute-resize-69606.jsonl", scenarios.resize_fails),
        ("execute-layout-changed.jsonl", scenarios.layout_changes),
        ("execute-root-read-back.jsonl", scenarios.read_back_corrupts),
    ):
        t = S.TempDir()
        try:
            r = scenarios.execute(t, before=before)
            scenarios.write_lines(os.path.join(out, name), r["channel"].lines)
            transcripts[name] = {"mode": "execute", "plan": canonical.digest(r["plan"]), "shrinks": True, "outcome": "Failed"}
        finally:
            t.cleanup()

    for name, kw in (
        ("startup-disk-retry.jsonl", {}),
        ("startup-disk-failed.jsonl", {"bless_mode": "fail"}),
        ("startup-disk-refused.jsonl", {"installation": "00000000-0000-4000-8000-000000000000"}),
    ):
        t = S.TempDir()
        try:
            r = scenarios.execute(t, before=scenarios.bless_fails)
            rr = scenarios.retry(r, t, **kw)
            scenarios.write_lines(os.path.join(out, name), rr["channel"].lines)
            outcome = "WaitingForRecovery" if rr["outcome"] == "waiting-for-recovery" else "Failed"
            transcripts[name] = {"mode": "startup-disk", "plan": canonical.digest(r["plan"]), "outcome": outcome}
        finally:
            t.cleanup()

    pretty(
        os.path.join(out, "strings.json"),
        {
            "schema": "sekka.test/1",
            "controls": "\x00\x01\x1f\x7f\b\f\n\r\t",
            "quotes": "\"\\/",
            "unicode": "é ß 漢字   \U0001f600",
            "keys": {"z": 1, "Z": 2, "é": 3, "\U0001f600": 4, "a": 5},
            "integers": [0, -1, 18446744073709551615, -9223372036854775808],
        },
    )
    pretty(os.path.join(out, "transcripts.json"), transcripts)
    rust = os.environ.get("SEKKA_CANONICALIZE")
    if rust:
        write_expected(rust, out)


PLATFORM_UUIDS = ("ABCDEF01-2345-4678-89AB-CDEF01234567", "00000000-0000-4000-8000-000000000000")


def write_expected(rust, out):
    """expected.json from the Rust crate's own canonical forms and digests
    (crates/contract/examples/canonicalize)."""
    import hashlib
    import subprocess

    documents = {}
    for name in sorted(os.listdir(out)):
        if not name.endswith(".json") or name in ("expected.json", "transcripts.json"):
            continue
        with open(os.path.join(out, name), "rb") as f:
            data = f.read()
        report = json.loads(subprocess.run([rust], input=data, capture_output=True, check=True).stdout)
        entry = {"canonical": report["canonical"]}
        entry["digest"] = "sha256:" + hashlib.sha256(report["canonical"].encode("utf-8")).hexdigest()
        if "digest" in report:
            assert report["digest"] == entry["digest"], name
        for key in ("layout_digest", "kernel_arguments"):
            if key in report:
                entry[key] = report[key]
        documents[name] = entry
    bindings = {}
    for platform in PLATFORM_UUIDS:
        stdout = subprocess.run([rust, platform], input=b'{"schema":"x/1"}', capture_output=True, check=True).stdout
        line = stdout.decode().splitlines()[0]
        bindings[platform] = line.rsplit(": ", 1)[1]
    report = json.loads(subprocess.run([rust], input=S.profile_bytes(), capture_output=True, check=True).stdout)
    pretty(
        os.path.join(out, "expected.json"),
        {"documents": documents, "machine_bindings": bindings, "profile_digest": report["digest"]},
    )


if __name__ == "__main__":
    main()
