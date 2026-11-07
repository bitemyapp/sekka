# SPDX-License-Identifier: GPL-3.0-or-later
"""Canonical JSON and the contract's digests, against canonical.rs and
vectors whose expected bytes and digests the Rust crate computed
(tests/vectors/expected.json; crates/contract/tests/engine_vectors.rs checks
the same file from the Rust side)."""

import json
import os
import unittest

import support  # noqa: F401  (puts the engine on sys.path)
from support import VECTORS
from sekka_engine import canonical
from sekka_engine import contract as c


class CanonicalRules(unittest.TestCase):
    def test_keys_are_sorted_and_compact(self):
        # canonical.rs keys_are_sorted_and_compact
        self.assertEqual(
            canonical.dumps({"b": 1, "a": {"d": [1, 2], "c": "x\ny"}}),
            b'{"a":{"c":"x\\ny","d":[1,2]},"b":1}',
        )

    def test_keys_sort_by_utf8_bytes(self):
        doc = {"é": 1, "z": 2, "Z": 3, "a": 4, "à": 5, "\U0001f600": 6, "￿": 7}
        keys = list(json.loads(canonical.dumps(doc)))
        self.assertEqual(keys, sorted(doc, key=lambda k: k.encode("utf-8")))

    def test_strings_escape_as_serde_json_does(self):
        cases = {
            "\x00": "\\u0000",
            "\x1f": "\\u001f",
            "\b\f\n\r\t": "\\b\\f\\n\\r\\t",
            '"\\': '\\"\\\\',
            "\x7f": "\x7f",
            "é \U0001f600": "é \U0001f600",
            "/": "/",
        }
        for raw, escaped in cases.items():
            self.assertEqual(canonical.dumps(raw), ('"' + escaped + '"').encode("utf-8"), repr(raw))

    def test_only_integers(self):
        for bad in (1.5, float("nan"), 1 << 64, -(1 << 63) - 1):
            with self.assertRaises(canonical.CanonicalError):
                canonical.dumps({"size": bad})
        self.assertEqual(canonical.dumps([(1 << 64) - 1, -(1 << 63), True, None]), b"[18446744073709551615,-9223372036854775808,true,null]")

    def test_lone_surrogates_are_refused(self):
        with self.assertRaises(canonical.CanonicalError):
            canonical.dumps("\ud800")

    def test_readers_require_canonical_bytes(self):
        # canonical.rs round_trip_requires_canonical_bytes
        doc = {"schema": "t/1", "zeta": 7, "alpha": "é"}
        data = canonical.dumps(doc)
        self.assertEqual(canonical.loads(data, 1024, "t/1"), doc)
        spaced = b'{"alpha":"\\u00e9", "schema":"t/1","zeta":7}'
        with self.assertRaises(canonical.NotCanonical):
            canonical.loads(spaced, 1024, "t/1")
        self.assertEqual(canonical.loads_relaxed(spaced, 1024, "t/1"), doc)
        for bad in (b'{"schema":"t/1","zeta":-0}', b'{"schema":"t/1","zeta":1.0}', b'{"schema":"t/1","z":NaN}'):
            with self.assertRaises(canonical.CanonicalError):
                canonical.loads(bad, 1024, "t/1")

    def test_schema_first_then_size_first(self):
        with self.assertRaises(canonical.UnsupportedSchema):
            canonical.loads(b'{"schema":"t/2","unexpected":true}', 1024, "t/1")
        with self.assertRaises(canonical.TooLarge):
            canonical.loads(b" " * 2048, 1024, "t/1")

    def test_depth_is_bounded(self):
        value = 0
        for _ in range(40):
            value = [value]
        with self.assertRaises(canonical.CanonicalError):
            canonical.dumps(value)


class Identifiers(unittest.TestCase):
    def test_versions(self):
        self.assertGreater(c.version_parts("14.10"), c.version_parts("14.8.3"))
        for bad in ("14", "14.08", "a.b", "1.2.3.4", ""):
            with self.assertRaises(c.Invalid):
                c.version(bad)

    def test_text(self):
        self.assertTrue(c.is_text("line\nline"))
        for bad in ("tab\there", "cr\r", "\x7f", "\x85", "x" * 2049):
            self.assertFalse(c.is_text(bad), repr(bad))
        cleaned = c.plain_text("Error\r\n\tcode -69606\x07" + "y" * 5000)
        self.assertTrue(c.is_text(cleaned))
        self.assertTrue(cleaned.startswith("…"))

    def test_uuids_and_slugs(self):
        self.assertEqual(c.uuid_any("910DE5CA-7390-4716-9A54-8028916E4D78"), "910de5ca-7390-4716-9a54-8028916e4d78")
        with self.assertRaises(c.Invalid):
            c.uuid("910DE5CA-7390-4716-9A54-8028916E4D78")
        c.slug("sekka-j514s-1c8b4e2c")
        for bad in ("", "-x", "Upper", "a" * 97, "é"):
            with self.assertRaises(c.Invalid):
                c.slug(bad)

    def test_timestamps(self):
        self.assertEqual(c.format_timestamp(1_800_000_000), "2027-01-15T08:00:00Z")
        c.timestamp("2028-02-29T23:59:59Z")
        for bad in ("2026-02-29T00:00:00Z", "2026-11-06T24:00:00Z", "2026-11-06 20:39:38Z"):
            with self.assertRaises(c.Invalid):
                c.timestamp(bad)

    def test_macos_stages(self):
        self.assertEqual(
            c.macos_stages(True),
            ["apple-inputs", "shrink", "stub-partition", "stub-system", "partitions", "firmware", "esp", "root", "recovery", "startup-disk"],
        )
        self.assertNotIn("shrink", c.macos_stages(False))


class Vectors(unittest.TestCase):
    """Bytes and digests the Rust crate computed for the same documents."""

    def setUp(self):
        with open(os.path.join(VECTORS, "expected.json")) as f:
            self.expected = json.load(f)

    def read(self, name):
        with open(os.path.join(VECTORS, name), "rb") as f:
            return f.read()

    def test_documents(self):
        self.assertTrue(self.expected["documents"])
        for name, want in self.expected["documents"].items():
            doc = json.loads(self.read(name))
            data = canonical.dumps(doc)
            self.assertEqual(data.decode("utf-8"), want["canonical"], name)
            self.assertEqual(canonical.digest_bytes(data), want["digest"], name)
            if "layout_digest" in want:
                self.assertEqual(c.layout_digest(doc["disk"]), want["layout_digest"], name)
                self.assertEqual(doc["layout_digest"], want["layout_digest"], name)
                c.check_inspection(doc)
            if doc.get("schema") == c.PLAN_SCHEMA:
                plan, digest = c.parse_plan(data)
                self.assertEqual(digest, want["digest"])
            if doc.get("schema") == c.HANDOFF_SCHEMA:
                self.assertEqual(c.kernel_arguments(doc["root"], doc["esp"]), want["kernel_arguments"])
            if doc.get("schema") == c.RECOVERY_SCHEMA:
                c.parse_recovery_config(data)

    def test_the_profile_digest_plans_bind(self):
        _, digest = c.load_profile(support.profile_bytes())
        self.assertEqual(digest, self.expected["profile_digest"])

    def test_machine_bindings(self):
        for platform, binding in self.expected["machine_bindings"].items():
            self.assertEqual(c.machine_binding(platform), binding)
            self.assertEqual(c.machine_binding(platform.lower()), binding)

    def test_free_space_plans_and_the_engine_keep_the_same_room_in_macos(self):
        # engine_vectors.rs holds plan.rs's MACOS_MARGIN to the same number.
        self.assertEqual(c.MACOS_MARGIN, self.expected["macos_margin"])

    def test_the_helper_and_the_engine_agree_on_what_refuses_the_owner(self):
        # engine_vectors.rs holds engine.rs's OWNER_REFUSALS to the same list.
        self.assertEqual(list(c.OWNER_REFUSALS), self.expected["owner_refusals"])


if __name__ == "__main__":
    unittest.main()
