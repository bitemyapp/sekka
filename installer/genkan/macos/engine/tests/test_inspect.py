# SPDX-License-Identifier: GPL-3.0-or-later
"""inspect: the contract's Inspection from Asahi's model of a fake Mac."""

import logging
import os
import plistlib
import unittest

import support as S
from sekka_engine import contract as c
from sekka_engine import inspect as engine_inspect

logging.disable(logging.CRITICAL)


def codes(doc, blocking=None):
    return sorted(i["code"] for i in doc["issues"] if blocking is None or i["blocking"] == blocking)


def details(doc, code):
    return [i["detail"] for i in doc["issues"] if i["code"] == code]


def developer(**kw):
    return lambda directory, **more: S.j514s_developer(directory, **kw, **more)


class InspectTests(unittest.TestCase):
    def setUp(self):
        self.t = S.TempDir()

    def tearDown(self):
        self.t.cleanup()

    def inspect(self, builder=S.j514s_clean, privileged=True, change=None):
        mac = builder(self.t.sub("mac"), privileged=privileged)
        if isinstance(mac, tuple):
            mac = mac[0]
        if change:
            change(mac)
        asahi, host = S.load_asahi(mac)
        outcome, doc, channel = S.inspect(mac, asahi, host, self.t.sub("work"), privileged=privileged)
        self.mac, self.channel = mac, channel
        return outcome, doc

    def test_an_inspection_for_an_owner_refuses_what_execute_would(self):
        """The helper's inspection before it opens an installation names the
        owner: the engine refuses, before it reports anything, every account
        and password execute and startup-disk refuse, with their codes, and
        otherwise reports the same inspection."""
        mac = S.j514s_clean(self.t.sub("mac"))
        asahi, host = S.load_asahi(mac)
        _, plain, _ = S.inspect(mac, asahi, host, self.t.sub("w-plain"))
        outcome, doc, _ = S.inspect(mac, asahi, host, self.t.sub("w-owner"), owner=S.OWNER)
        self.assertEqual((outcome, doc), ("inspected", plain))
        before = mac.state_digest()
        for i, (owner, password, code) in enumerate(S.UNUSABLE_OWNERS):
            with self.subTest(owner=owner, password=password):
                outcome, doc, channel = S.inspect(mac, asahi, host, self.t.sub(f"w{i}"), owner=owner, password=password)
                self.assertEqual((outcome, doc), ("failed", None))
                [failure] = [m["message"]["payload"] for m in channel.messages() if m["message"]["type"] == "failure"]
                self.assertEqual((failure["stage"], failure["failure"]["code"], failure["failure"]["disk_changed"]), (None, code, "no"))
                self.assertIn(code, c.OWNER_REFUSALS)
                self.assertNotIn(S.PASSWORD, b"\n".join(channel.lines))
        self.assertEqual(mac.state_digest(), before)
        self.assertEqual(mac.mounted_now(), [])

    def test_a_clean_j514s(self):
        outcome, doc = self.inspect()
        self.assertEqual(outcome, "inspected")
        c.check_inspection(doc)
        self.assertEqual(doc["machine"]["binding"], c.machine_binding(self.mac.platform_uuid))
        self.assertEqual(doc["machine"]["device_class"], "j514sap")
        self.assertEqual(doc["disk"]["uuid"], self.mac.disk_uuid.lower())
        self.assertEqual([p["kind"] for p in doc["disk"]["partitions"]], ["apple-isc", "apfs", "apple-recovery"])
        self.assertEqual([p["name"] for p in doc["disk"]["partitions"]], ["iBootSystemContainer", "Container", "RecoveryOSContainer"])
        self.assertEqual(doc["installations"], [])
        self.assertEqual(codes(doc, blocking=True), [])
        self.assertEqual(codes(doc), ["known-macos-issue"])
        self.assertEqual(doc["filevault"], "on")
        self.assertEqual(doc["power"], {"has_battery": True, "on_ac": True, "battery_percent": 80})
        self.assertTrue(doc["macos"]["internal_boot"])
        [k] = doc["disk"]["containers"]
        self.assertEqual(k["volume_groups"][0]["os"], {"kind": "macos", "version": "27.0.1"})
        self.assertEqual(k["volume_groups"][0]["name"], "Macintosh HD")
        # Asahi's rule: macOS keeps 38 GB free beyond what it uses, and never
        # less than diskutil's preferred minimum.
        used = S.CONTAINER_CLEAN - k["free"]
        minimum = -(-(used + 38_000_000_000) // S.MIB) * S.MIB
        self.assertEqual(k["resize"]["minimum"], max(minimum, S.PREFERRED))
        [cand] = doc["candidates"]
        self.assertEqual(cand["kind"], "shrink-container")
        self.assertEqual(cand["available"], S.CONTAINER_CLEAN - k["resize"]["minimum"])
        self.assertEqual(cand["adjacent_free"], 0)

    def test_unprivileged_inspection_reads_only_and_says_what_it_couldnt_read(self):
        outcome, doc = self.inspect(privileged=False)
        self.assertEqual(outcome, "inspected")
        self.assertEqual(codes(doc, blocking=True), [])
        logs = [m["message"]["payload"]["text"] for m in self.channel.messages() if m["message"]["type"] == "log"]
        self.assertTrue(any("bputil" in text for text in logs), logs)
        mounts = [cmd for cmd in self.mac.commands if cmd[:2] == ["diskutil", "mount"]]
        self.assertTrue(mounts)
        self.assertTrue(all(cmd[2] == "readOnly" for cmd in mounts), mounts)
        self.assertEqual(self.mac.mounted_now(), [], "everything the inspection mounted is unmounted again")

    def test_a_root_inspection_mounts_only_read_only_and_undoes_it(self):
        """As root too: other systems' volumes and ESPs (here an Asahi
        installation's) are mounted read-only, and nothing is left mounted."""
        for builder in (S.j514s_clean, S.m1_pro_with_asahi, S.j514s_developer, lambda root, **kw: S.j514s_existing_sekka(root, **kw)[0]):
            with self.subTest(builder=getattr(builder, "__name__", "existing")):
                self.t.cleanup()
                self.t = S.TempDir()
                outcome, doc = self.inspect(builder, privileged=True)
                self.assertEqual(outcome, "inspected")
                self.assertTrue(self.mac.mount_log)
                self.assertEqual({mode for _, mode in self.mac.mount_log}, {"ro"})
                self.assertEqual(self.mac.mounted_now(), [])

    def test_free_space_is_offered_beside_the_shrink(self):
        outcome, doc = self.inspect(S.j514s_free_space)
        free = [k for k in doc["candidates"] if k["kind"] == "free-space"]
        self.assertEqual(free, [{"kind": "free-space", "extent": {"offset": CONTAINER_END, "size": S.RECOVERY[0] - CONTAINER_END}}])
        shrink = [k for k in doc["candidates"] if k["kind"] == "shrink-container"][0]
        self.assertEqual(shrink["adjacent_free"], S.RECOVERY[0] - CONTAINER_END)

    def test_an_existing_sekka_is_recognized_and_stops_installation(self):
        for enrolled, privileged, state in ((None, True, "waiting-for-recovery"), ("abc123", True, "complete"), (None, False, "unknown")):
            with self.subTest(enrolled=enrolled, privileged=privileged):
                self.t.cleanup()
                self.t = S.TempDir()
                outcome, doc = self.inspect(lambda root, **kw: S.j514s_existing_sekka(root, enrolled=enrolled, **kw), privileged=privileged)
                [inst] = doc["installations"]
                self.assertEqual(inst["kind"], "sekka")
                self.assertEqual(inst["state"], state)
                self.assertEqual(inst["installation_id"], "8e2f6c1a-6b3d-4c1e-9a55-0d4f2f2b7c11")
                self.assertIn("existing-sekka", codes(doc, blocking=True))
                self.assertEqual([p["kind"] for p in doc["disk"]["partitions"]], ["apple-isc", "apfs", "apfs", "efi", "linux", "apple-recovery"])
                self.assertEqual(doc["disk"]["partitions"][3]["name"], "EFI - SEKKA")
                self.assertEqual(doc["disk"]["partitions"][2]["offset"], S.STUB[0])

    def test_a_product_install_is_identified_by_its_handoff(self):
        """Its handoff and Recovery configuration name it, also on an ESP
        with the developer image's serial, which a release whose installer
        data names that serial gives it."""
        for serial in (None, S.DEVELOPER_SERIAL):
            with self.subTest(serial=serial):
                self.t.cleanup()
                self.t = S.TempDir()
                outcome, doc = self.inspect(lambda root, **kw: S.j514s_existing_sekka(root, esp_serial=serial, **kw)[0])
                [inst] = doc["installations"]
                self.assertEqual((inst["kind"], inst["installation_id"]), ("sekka", "8e2f6c1a-6b3d-4c1e-9a55-0d4f2f2b7c11"))
                [detail] = details(doc, "existing-sekka")
                self.assertTrue(detail.startswith("Sekka is already installed on this Mac (Sekka)."), detail)
                self.assertNotIn("developer", detail)

    def test_msdosfs_uuids_are_the_ones_macos_reports(self):
        """diskutil names a FAT volume by the UUID msdosfs derives from its
        boot sector's serial and sector count, not by the serial. The
        recorded M1 Pro's ESP (500 MB of 4 KiB blocks) has the UUID of serial
        CEF8-B598: of every 32-bit serial, that one gives it, which no wrong
        derivation would. The fake reports UUIDs the same way."""
        path = os.path.join(S.FIXTURES, "omarchy-m1-pro-2026-09-28", "legacy", "info", "disk0s4.plist")
        with open(path, "rb") as f:
            recorded = plistlib.load(f)
        sectors = recorded["Size"] // recorded["DeviceBlockSize"]
        self.assertEqual(sectors, 128000)
        self.assertEqual(engine_inspect.fat_volume_uuid(0xCEF8B598, sectors), c.uuid_any(recorded["VolumeUUID"]))
        self.assertEqual(S.msdosfs_uuid(0xCEF8B598, sectors), recorded["VolumeUUID"])
        mac = S.m1_pro_with_asahi(self.t.sub("mac"))
        self.assertEqual(mac.info("disk0s4")["VolumeUUID"], recorded["VolumeUUID"])
        # The developer image's serial, on a 1 GiB ESP of 4 KiB blocks.
        self.assertEqual(engine_inspect.DEVELOPER_ESP_SERIAL, S.DEVELOPER_SERIAL)
        self.assertEqual(engine_inspect.fat_volume_uuid(S.DEVELOPER_SERIAL, S.GIB // 4096), c.uuid_any(S.msdosfs_uuid(S.DEVELOPER_SERIAL, S.GIB // 4096)))

    def test_a_developer_install_is_sekka_and_stops_installation(self):
        """Sekka's developer image, installed by install-sekka.sh as on the
        reference J514S, has no handoff and no Recovery configuration. Its
        ESP's serial names it: it is Sekka, with no installation ID, and the
        blocking existing-sekka issue says what it is and how to remove it,
        older images without boot-bin's record too. Looking at it reads its
        volumes read-only and changes nothing."""
        cases = (
            ({"enrolled": "deadbeef"}, True, "complete"),
            ({"enrolled": None}, True, "waiting-for-recovery"),
            ({"enrolled": "deadbeef"}, False, "unknown"),
            ({"files": {k: v for k, v in S.SEKKA_ESP_FILES.items() if k != "sekka/boot-bin.json"}}, True, "complete"),
        )
        for kw, privileged, state in cases:
            with self.subTest(kw=list(kw), privileged=privileged):
                self.t.cleanup()
                self.t = S.TempDir()
                mac = S.j514s_developer(self.t.sub("mac"), privileged=privileged, **kw)
                before = mac.state_digest()
                outcome, doc = self.inspect(lambda root, **_: mac, privileged=privileged)
                self.assertEqual(outcome, "inspected")
                [inst] = doc["installations"]
                self.assertEqual((inst["kind"], inst["state"], inst["installation_id"], inst["name"]), ("sekka", state, None, "Sekka"))
                parts = {p["kind"]: p["uuid"] for p in doc["disk"]["partitions"] if p["kind"] in ("efi", "linux")}
                self.assertEqual((inst["esp"], inst["root"]), (parts["efi"], parts["linux"]))
                self.assertIsNotNone(inst["stub_group"])
                self.assertIn("existing-sekka", codes(doc, blocking=True))
                self.assertNotIn("partial-installation", codes(doc))
                [detail] = details(doc, "existing-sekka")
                self.assertTrue(detail.startswith("A developer installation of Sekka, as install-sekka.sh makes, is on this Mac (Sekka)"), detail)
                self.assertIn("disk0s4 has the developer image's serial, 5ECC-A5E1", detail)
                self.assertIn("delete disk0s3, disk0s4 and disk0s5 with diskutil", detail)
                self.assertIn("docs/apple-silicon/removal-and-boards.md", detail)
                self.assertEqual({mode for _, mode in mac.mount_log}, {"ro"})
                self.assertEqual(mac.mounted_now(), [])
                self.assertEqual(mac.state_digest(), before)

    def test_a_developer_install_beside_free_space_still_stops_installation(self):
        """The free space beside it is described, as any is, and not offered:
        the developer installation stops installation."""
        outcome, doc = self.inspect(developer(free_space=True))
        [free] = [k for k in doc["candidates"] if k["kind"] == "free-space"]
        root = [p for p in doc["disk"]["partitions"] if p["kind"] == "linux"][0]
        self.assertEqual(free["extent"], {"offset": root["offset"] + root["size"], "size": S.RECOVERY[0] - root["offset"] - root["size"]})
        [inst] = doc["installations"]
        self.assertEqual((inst["kind"], inst["installation_id"]), ("sekka", None))
        self.assertIn("existing-sekka", codes(doc, blocking=True))

    def test_a_developer_install_missing_parts_is_partial(self):
        cases = (
            ({"root": False}, "no Linux partition follows it"),
            ({"files": {}}, "disk0s4 holds none of its boot files"),
        )
        for kw, missing in cases:
            with self.subTest(missing=missing):
                self.t.cleanup()
                self.t = S.TempDir()
                outcome, doc = self.inspect(developer(**kw))
                [inst] = doc["installations"]
                self.assertEqual((inst["kind"], inst["state"], inst["installation_id"]), ("sekka", "partial", None))
                self.assertEqual(codes(doc, blocking=True), ["partial-installation"])
                [detail] = details(doc, "partial-installation")
                self.assertIn("incomplete developer installation of Sekka", detail)
                self.assertIn(missing, detail)

    def test_a_developer_install_stopped_before_its_stub_is_partial(self):
        """The stub's container holds no system; the ESP after it has the
        developer image's serial, which the issue says."""
        outcome, doc = self.inspect(lambda root, **kw: S.j514s_developer(root, stub="empty", **kw))
        [inst] = doc["installations"]
        self.assertEqual((inst["kind"], inst["state"], inst["installation_id"]), ("sekka", "partial", None))
        self.assertEqual(codes(doc, blocking=True), ["partial-installation"])
        [detail] = details(doc, "partial-installation")
        self.assertIn("the EFI partition after it, disk0s4, has the developer image's serial, 5ECC-A5E1", detail)

    def test_what_a_removed_developer_install_left_stops_installation(self):
        """Its ESP, which no startup volume claims, still has the developer
        image's serial. The root beside it can't be read from macOS and stays
        unknown."""
        outcome, doc = self.inspect(developer(stub=False))
        kinds = sorted((i["kind"], i["state"]) for i in doc["installations"])
        self.assertEqual(kinds, [("sekka", "partial"), ("unknown", "partial")])
        self.assertEqual(codes(doc, blocking=True), ["partial-installation"])
        [detail] = details(doc, "partial-installation")
        self.assertIn("The EFI partition disk0s3 has the developer image's serial, 5ECC-A5E1", detail)
        self.assertEqual(codes(doc, blocking=False).count("unrecognized-layout"), 1)

    def test_sekkas_boot_record_without_an_identity_is_not_guessed(self):
        """An ESP with Sekka's boot record but neither a handoff nor the
        developer image's serial is Sekka's, but which installation it is
        isn't guessed: its state is unknown, and installation stops."""
        outcome, doc = self.inspect(developer(serial=0x12345678))
        [inst] = doc["installations"]
        self.assertEqual((inst["kind"], inst["state"], inst["installation_id"]), ("sekka", "unknown", None))
        [detail] = details(doc, "existing-sekka")
        self.assertIn("can't tell which Sekka installation it is", detail)
        self.assertIn("existing-sekka", codes(doc, blocking=True))
        self.t.cleanup()
        self.t = S.TempDir()
        outcome, doc = self.inspect(developer(serial=0x12345678, stub=False))
        self.assertIn(("sekka", "partial"), [(i["kind"], i["state"]) for i in doc["installations"]])
        self.assertEqual(codes(doc, blocking=True), ["partial-installation"])

    def test_another_asahi_install_on_a_j514s_is_other_linux(self):
        """Asahi's ESP, with a serial of its own and none of Sekka's files,
        is another system: it stops nothing."""
        outcome, doc = self.inspect(S.j514s_asahi)
        [inst] = doc["installations"]
        self.assertEqual((inst["kind"], inst["state"], inst["name"]), ("other-linux", "complete", "Asahi Linux"))
        self.assertEqual(codes(doc, blocking=True), [])

    def test_a_partial_installation_stops_without_a_guess(self):
        outcome, doc = self.inspect(S.j514s_partial)
        [inst] = doc["installations"]
        self.assertEqual((inst["kind"], inst["state"], inst["stub_group"]), ("unknown", "partial", None))
        self.assertEqual(codes(doc, blocking=True), ["partial-installation"])
        macos, stub = sorted(doc["disk"]["containers"], key=lambda k: k["size"], reverse=True)
        self.assertEqual(stub["volume_groups"], [])
        self.assertEqual(stub["size"], S.STUB[1])

    def test_unknown_systems_are_described_and_left_alone(self):
        outcome, doc = self.inspect(S.j514s_unknown)
        kinds = sorted((i["kind"], i["state"]) for i in doc["installations"])
        self.assertEqual(kinds, [("unknown", "partial"), ("unknown", "unknown")])
        self.assertEqual(codes(doc, blocking=True), [])
        self.assertEqual(codes(doc, blocking=False).count("unrecognized-layout"), 2)
        group = [g for k in doc["disk"]["containers"] for g in k["volume_groups"] if g["name"] == "Mystery"]
        self.assertEqual(group[0]["os"], {"kind": "stub", "for_system": "Mystery"})

    def test_an_m1_pro_with_asahi_is_refused_but_described(self):
        outcome, doc = self.inspect(S.m1_pro_with_asahi)
        self.assertEqual(outcome, "inspected")
        self.assertIn("unsupported-machine", codes(doc, blocking=True))
        self.assertEqual(doc["machine"]["device_class"], "j314sap")
        [inst] = doc["installations"]
        self.assertEqual((inst["kind"], inst["state"], inst["name"]), ("other-linux", "complete", "Asahi Alarm Minimal"))
        # The recorded identities, from Omarchy's fixture.
        self.assertEqual(inst["esp"], "1bc56391-63e4-4bbd-9485-4517bc7f3aa7")
        self.assertEqual(inst["root"], "362cb451-0d83-46a6-a9a4-e68760757d3e")
        self.assertEqual(inst["stub_group"], "0bf26a61-4aff-49ed-814f-9323ee7602a4")
        names = [p["name"] for p in doc["disk"]["partitions"]]
        self.assertIn("EFI - ASAHI", names)

    def test_untested_firmware_is_refused(self):
        def change(mac):
            mac.sys_firmware = "mBoot-20458.1.1"

        outcome, doc = self.inspect(change=change)
        self.assertIn("untested-firmware", codes(doc, blocking=True))

    def test_power_filevault_snapshots_and_disks(self):
        cases = [
            (lambda m: setattr(m, "pmset", "Now drawing from 'Battery Power'\n -InternalBattery-0 (id=1)\t55%; discharging\n"), "on-battery", True),
            (lambda m: setattr(m, "pmset", "Now drawing from 'AC Power'\n -InternalBattery-0 (id=1)\t9%; charging\n"), "battery-low", False),
            (lambda m: setattr(m, "fdesetup", "Encryption in progress: Percent completed = 40.2\n"), "file-vault-converting", True),
            (lambda m: setattr(m, "extra_internal_disks", 1), "ambiguous-disk", True),
        ]
        for change, code, blocking in cases:
            with self.subTest(code=code):
                self.t.cleanup()
                self.t = S.TempDir()
                outcome, doc = self.inspect(change=change)
                self.assertEqual(outcome, "inspected")
                self.assertIn((code, blocking), [(i["code"], i["blocking"]) for i in doc["issues"]])

    def test_snapshots_are_counted_and_explained(self):
        def change(mac):
            _, data = [(ct, v) for ct in mac.containers.values() for v in ct.volumes if v.name == "Macintosh HD - Data"][0]
            data.snapshots = [{"SnapshotName": "com.apple.TimeMachine.2026-11-01-120000.local", "LimitingContainerShrink": True}]

        outcome, doc = self.inspect(change=change)
        macos = [k for k in doc["disk"]["containers"] if k["resize"]][0]
        self.assertEqual(macos["snapshots"], 1)
        self.assertIn(("local-snapshots", False), [(i["code"], i["blocking"]) for i in doc["issues"]])

    def test_without_resize_limits_no_shrink_is_offered(self):
        def change(mac):
            mac.limits.clear()

        outcome, doc = self.inspect(change=change)
        self.assertEqual(doc["candidates"], [])
        self.assertIn(("resize-limits-unavailable", False), [(i["code"], i["blocking"]) for i in doc["issues"]])
        self.assertIn("not-enough-space", codes(doc, blocking=True))

    def test_an_unreadable_mac_fails_through_the_transcript(self):
        def change(mac):
            mac.platform_uuid = "not-a-uuid"

        outcome, doc = self.inspect(change=change)
        self.assertEqual(outcome, "failed")
        self.assertIsNone(doc)
        kinds = [m["message"]["type"] for m in self.channel.messages()]
        self.assertEqual(kinds, ["hello", "failure", "completion"])
        failure = self.channel.messages()[1]["message"]["payload"]
        self.assertEqual((failure["stage"], failure["failure"]["code"]), (None, "inspection-failed"))

    def test_inspection_changes_nothing(self):
        mac = S.j514s_existing_sekka(self.t.sub("mac"))[0]
        before = mac.state_digest()
        asahi, host = S.load_asahi(mac)
        S.inspect(mac, asahi, host, self.t.sub("work"))
        self.assertEqual(mac.state_digest(), before)
        mutating = {"addPartition", "resizeContainer", "addVolume", "changeVolumeRole", "rename", "eraseVolume"}
        for cmd in mac.commands:
            self.assertFalse(mutating & set(cmd) and "limits" not in cmd, cmd)
            self.assertNotIn(cmd[0], ("newfs_msdos", "bless", "chflags", "xattr"), cmd)


CONTAINER_END = S.CONTAINER_OFFSET + S.CONTAINER_AFTER


if __name__ == "__main__":
    unittest.main()
