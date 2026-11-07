# SPDX-License-Identifier: GPL-3.0-or-later
"""The release's files: Recovery-app zip members, the OS package and Asahi's
template, refused before anything changes when they aren't what they must
be."""

import json
import logging
import os
import stat
import unittest
import zipfile

import scenarios
import support as S
from sekka_engine import assets as A
from sekka_engine import canonical
from sekka_engine import contract as c

logging.disable(logging.CRITICAL)

APP = "Sekka Recovery.app/"
GOOD = [
    (APP, b"", 0o40755),
    (APP + "Contents/", b"", 0o40755),
    (APP + "Contents/Info.plist", b"<plist/>", 0o100644),
    (APP + "Contents/MacOS/", b"", 0o40755),
    (APP + "Contents/MacOS/Sekka Recovery", b"\xcf\xfa\xed\xfe", 0o100755),
]


def read(path):
    with open(path, "rb") as f:
        return f.read()


def write(path, data):
    with open(path, "wb") as f:
        f.write(data)


def recovery_zip(t, members, edit=None):
    path = os.path.join(t.sub("zips"), f"r{len(os.listdir(t.sub('zips')))}.zip")
    with open(path, "wb") as f:
        f.write(S.stored_zip(members))
    if edit:
        edit(path)
    return path


class RecoveryZipTests(unittest.TestCase):
    def setUp(self):
        self.t = S.TempDir()

    def tearDown(self):
        self.t.cleanup()

    def test_a_good_app_is_accepted_with_its_inventory(self):
        zf, members = A.check_recovery_zip(recovery_zip(self.t, GOOD))
        A.close_zip(zf)
        digest, files = A.recovery_inventory(members)
        self.assertEqual([f["path"] for f in files], [APP + "Contents/Info.plist", APP + "Contents/MacOS/Sekka Recovery"])
        self.assertEqual(files[1]["digest"], canonical.digest_bytes(b"\xcf\xfa\xed\xfe"))
        modes = {i.filename: m for i, k, m, s, d in members}
        self.assertEqual(modes[APP + "Contents/MacOS/Sekka Recovery"], 0o755)
        self.assertEqual(modes[APP + "Contents/Info.plist"], 0o644)

    def test_attacks_are_refused(self):
        cases = {
            "traversal": GOOD + [(APP + "../evil", b"x", 0o100644)],
            "deep traversal": GOOD + [(APP + "Contents/../../../etc/x", b"x", 0o100644)],
            "absolute": GOOD + [("/etc/passwd", b"x", 0o100644)],
            "outside the app": GOOD + [("Other.app/Contents/x", b"x", 0o100644)],
            "dot component": GOOD + [(APP + "./Contents/x", b"x", 0o100644)],
            "empty component": GOOD + [(APP + "Contents//x", b"x", 0o100644)],
            "symlink": GOOD + [(APP + "Contents/link", b"/etc/passwd", stat.S_IFLNK | 0o777)],
            "device": GOOD + [(APP + "Contents/dev", b"", stat.S_IFCHR | 0o600)],
            "resource fork": GOOD + [("__MACOSX/" + APP + "._Info.plist", b"x", 0o100644)],
            "appledouble": GOOD + [(APP + "Contents/._Info.plist", b"x", 0o100644)],
            "case twin": GOOD + [(APP + "contents/info.plist", b"x", 0o100644)],
            "backslash": GOOD + [(APP + "Contents\\x", b"x", 0o100644)],
            "no executable": GOOD[:3],
            "no Info.plist": [m for m in GOOD if not m[0].endswith("Info.plist")],
        }
        for name, members in cases.items():
            with self.subTest(name):
                with self.assertRaises(A.AssetError):
                    zf, _ = A.check_recovery_zip(recovery_zip(self.t, members))
                    A.close_zip(zf)

    def test_encrypted_and_corrupt_members_are_refused(self):
        def encrypt(path):
            data = bytearray(read(path))
            zf = zipfile.ZipFile(path)
            info = zf.getinfo(APP + "Contents/Info.plist")
            zf.close()
            for offset in (info.header_offset + 6,):
                data[offset] |= 1
            # Mark it in the central directory too.
            pos = 0
            while True:
                pos = data.find(b"PK\x01\x02", pos)
                if pos < 0:
                    break
                name_len = int.from_bytes(data[pos + 28 : pos + 30], "little")
                if data[pos + 46 : pos + 46 + name_len] == (APP + "Contents/Info.plist").encode():
                    data[pos + 8] |= 1
                pos += 4
            write(path, bytes(data))

        def corrupt(path):
            data = bytearray(read(path))
            data[data.find(b"\xcf\xfa\xed\xfe")] ^= 0xFF
            write(path, bytes(data))

        for name, edit in (("encrypted", encrypt), ("corrupt", corrupt), ("not a zip", lambda p: write(p, b"no"))):
            with self.subTest(name):
                with self.assertRaises((A.AssetError, zipfile.BadZipFile)):
                    zf, _ = A.check_recovery_zip(recovery_zip(self.t, GOOD, edit))
                    A.close_zip(zf)

    def test_an_attack_is_refused_before_the_disk_changes(self):
        run = scenarios.execute(
            self.t,
            release_kw={"recovery_members": GOOD + [(APP + "../../.IAPhysicalMedia", b"x", 0o100644)]},
        )
        messages = run["channel"].messages()
        failure = messages[-2]["message"]["payload"]
        self.assertEqual((failure["stage"], failure["failure"]["code"]), (None, "asset-mismatch"))
        self.assertEqual(run["mac"].state_digest(), run["state_before"])


class PayloadAndTemplateTests(unittest.TestCase):
    def setUp(self):
        self.t = S.TempDir()

    def tearDown(self):
        self.t.cleanup()

    def bound(self, edit):
        """Edit a release file and bind the plan to the edited bytes."""
        mac = S.j514s_clean(self.t.sub("mac"))
        asahi, host = S.load_asahi(mac)
        _, doc, _ = S.inspect(mac, asahi, host, self.t.sub("w0"))
        release = S.make_release(self.t.sub("assets"))
        edit(release["dir"])
        for name in ("payload.zip", "installer_data.json", "manifest.json", "recovery.zip"):
            with open(os.path.join(release["dir"], name), "rb") as f:
                release["digests"][name] = canonical.digest_bytes(f.read())
        plan = S.derive_plan(doc, release, 190 * S.GIB)
        before = mac.state_digest()
        outcome, channel = S.run_engine("execute", mac, asahi, host, self.t.sub("w"), plan=plan, release=release)
        failure = channel.messages()[-2]["message"]["payload"]
        self.assertEqual((outcome, failure["stage"], failure["failure"]["code"]), ("failed", None, "asset-mismatch"))
        self.assertEqual(mac.state_digest(), before)
        return failure["failure"]["message"]

    def edit_json(self, name, change):
        def edit(directory):
            path = os.path.join(directory, name)
            with open(path) as f:
                doc = json.load(f)
            change(doc)
            with open(path, "w") as f:
                json.dump(doc, f)

        return edit

    def test_template_must_be_exactly_the_esp_and_root(self):
        def extras(doc):
            doc["os_list"][0]["extras"] = ["https://example.com/x"]

        def second_os(doc):
            doc["os_list"].append(doc["os_list"][0])

        def wrong_firmware(doc):
            doc["os_list"][0]["supported_fw"] = ["13.5"]

        def small_esp(doc):
            doc["os_list"][0]["partitions"][0]["size"] = "500MB"

        def no_firmware(doc):
            doc["os_list"][0]["partitions"][0]["copy_firmware"] = False

        for change in (extras, second_os, wrong_firmware, small_esp, no_firmware):
            with self.subTest(change.__name__):
                self.t.cleanup()
                self.t = S.TempDir()
                self.bound(self.edit_json("installer_data.json", change))

    def test_manifest_must_match_the_payload(self):
        def wrong_root(doc):
            doc["root_image"]["digest"] = canonical.digest_bytes(b"other")

        def missing_file(doc):
            doc["esp"]["files"] = doc["esp"]["files"][1:]

        def wrong_file(doc):
            doc["esp"]["files"][0]["digest"] = canonical.digest_bytes(b"other")

        def other_release(doc):
            doc["release"] = "sekka-j514s-00000000"

        for change in (wrong_root, missing_file, wrong_file, other_release):
            with self.subTest(change.__name__):
                self.t.cleanup()
                self.t = S.TempDir()
                self.bound(self.edit_json("manifest.json", change))

    def test_payload_members_outside_esp_and_root_are_refused(self):
        def edit(directory):
            path = os.path.join(directory, "payload.zip")
            with zipfile.ZipFile(path, "a") as zf:
                zf.writestr("esp/../../escape", b"x")

        message = self.bound(edit)
        self.assertIn("escape", message)

    def test_a_boot_entry_without_placeholders_is_refused(self):
        def edit(directory):
            path = os.path.join(directory, "payload.zip")
            members = []
            with zipfile.ZipFile(path) as zf:
                for info in zf.infolist():
                    data = zf.read(info)
                    if info.filename == "esp/" + S.BOOT_ENTRY:
                        data = data.replace(b"@SEKKA_ESP_PARTUUID@", b"by-label")
                    members.append((info.filename, data, info.external_attr >> 16))
            with open(path, "wb") as f:
                f.write(S.stored_zip(members))
            mpath = os.path.join(directory, "manifest.json")
            with open(mpath) as f:
                doc = json.load(f)
            for item in doc["esp"]["files"]:
                if item["path"] == S.BOOT_ENTRY:
                    with zipfile.ZipFile(path) as zf:
                        data = zf.read("esp/" + S.BOOT_ENTRY)
                    item["size"], item["digest"] = len(data), canonical.digest_bytes(data)
            with open(mpath, "w") as f:
                json.dump(doc, f)

        mac = S.j514s_clean(self.t.sub("mac"))
        asahi, host = S.load_asahi(mac)
        _, doc, _ = S.inspect(mac, asahi, host, self.t.sub("w0"))
        release = S.make_release(self.t.sub("assets"))
        edit(release["dir"])
        for name in ("payload.zip", "manifest.json"):
            with open(os.path.join(release["dir"], name), "rb") as f:
                release["digests"][name] = canonical.digest_bytes(f.read())
        plan = S.derive_plan(doc, release, 190 * S.GIB)
        outcome, channel = S.run_engine("execute", mac, asahi, host, self.t.sub("w"), plan=plan, release=release)
        failure = channel.messages()[-2]["message"]["payload"]["failure"]
        self.assertEqual(failure["code"], "asset-mismatch")
        self.assertIn("@SEKKA_ESP_PARTUUID@", failure["message"])

    def test_a_boot_entry_that_doesnt_pass_the_partitions_is_refused(self):
        """Each placeholder must be the value of its own argument to the
        kernel: that is how first-boot setup finds the partitions."""
        entry = (
            b"title Sekka\nlinux /EFI/nixos/kernel.efi\n"
            b"options init=/nix/store/x/init root=PARTUUID=@SEKKA_ROOT_PARTUUID@ sekka.esp=PARTUUID=@SEKKA_ESP_PARTUUID@\n"
        )
        mac = S.j514s_clean(self.t.sub("mac"))
        asahi, host = S.load_asahi(mac)
        _, doc, _ = S.inspect(mac, asahi, host, self.t.sub("w0"))
        release = S.make_release(self.t.sub("assets"), boot_entry=entry)
        plan = S.derive_plan(doc, release, 190 * S.GIB)
        before = mac.state_digest()
        outcome, channel = S.run_engine("execute", mac, asahi, host, self.t.sub("w"), plan=plan, release=release)
        failure = channel.messages()[-2]["message"]["payload"]["failure"]
        self.assertEqual(failure["code"], "asset-mismatch")
        self.assertIn("sekka.root=PARTUUID=@SEKKA_ROOT_PARTUUID@", failure["message"])
        self.assertEqual(mac.state_digest(), before)

    def test_the_partitions_are_named_anywhere_on_the_command_line(self):
        """handoff.rs's names_partitions, case for case."""
        root, esp = "63d89be1-72f7-4aff-9679-09cafa7e0c2d", "910de5ca-7390-4716-9a54-8028916e4d78"
        apart = (
            "title Sekka\noptions sekka.esp=PARTUUID=910DE5CA-7390-4716-9A54-8028916E4D78 init=/x\n"
            "options quiet  sekka.root=PARTUUID=63d89be1-72f7-4aff-9679-09cafa7e0c2d\n"
        )
        self.assertTrue(c.names_partitions(apart, root, esp))
        self.assertEqual(
            c.partition_arguments(apart),
            "sekka.esp=PARTUUID=910DE5CA-7390-4716-9A54-8028916E4D78 sekka.root=PARTUUID=63d89be1-72f7-4aff-9679-09cafa7e0c2d",
        )
        for entry in (
            f"options sekka.root=PARTUUID={root}\n",
            f"options sekka.root=PARTUUID={root}x sekka.esp=PARTUUID={esp}\n",
            f"linux sekka.root=PARTUUID={root} sekka.esp=PARTUUID={esp}\n",
        ):
            self.assertFalse(c.names_partitions(entry, root, esp), entry)
        for entry in (
            "options root=PARTUUID=@SEKKA_ROOT_PARTUUID@ sekka.esp=PARTUUID=@SEKKA_ESP_PARTUUID@\n",
            "title @SEKKA_ROOT_PARTUUID@\noptions sekka.esp=PARTUUID=@SEKKA_ESP_PARTUUID@\n",
            "options sekka.root=PARTUUID=@SEKKA_ROOT_PARTUUID@,sekka.esp=PARTUUID=@SEKKA_ESP_PARTUUID@\n",
        ):
            with self.assertRaises(c.Invalid, msg=entry):
                c.fill_entry(entry, root, esp)


class AppleDoubleTests(unittest.TestCase):
    def test_only_real_sidecars_with_companions_are_removed(self):
        from sekka_engine.engine import remove_appledouble_sidecars

        t = S.TempDir()
        try:
            root = t.sub("esp")
            write(os.path.join(root, "kernel.efi"), b"k")
            write(os.path.join(root, "._kernel.efi"), b"\x00\x05\x16\x07rest")
            write(os.path.join(root, "._."), b"\x00\x05\x16\x07")
            write(os.path.join(root, "._orphan"), b"\x00\x05\x16\x07")
            write(os.path.join(root, "._notdouble"), b"plain")
            write(os.path.join(root, "notdouble"), b"x")
            self.assertEqual(remove_appledouble_sidecars(root), 2)
            self.assertEqual(sorted(os.listdir(root)), ["._notdouble", "._orphan", "kernel.efi", "notdouble"])
        finally:
            t.cleanup()


if __name__ == "__main__":
    unittest.main()
