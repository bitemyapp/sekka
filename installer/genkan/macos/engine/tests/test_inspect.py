# SPDX-License-Identifier: GPL-3.0-or-later
"""inspect: the contract's Inspection from Asahi's model of a fake Mac."""

import logging
import unittest

import support as S
from sekka_engine import contract as c

logging.disable(logging.CRITICAL)


def codes(doc, blocking=None):
    return sorted(i["code"] for i in doc["issues"] if blocking is None or i["blocking"] == blocking)


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
        for builder in (S.j514s_clean, S.m1_pro_with_asahi, lambda root, **kw: S.j514s_existing_sekka(root, **kw)[0]):
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
