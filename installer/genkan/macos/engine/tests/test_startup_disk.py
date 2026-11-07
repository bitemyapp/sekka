# SPDX-License-Identifier: GPL-3.0-or-later
"""startup-disk: the helper's bounded retry after execute failed to set the
startup disk. Only that stage runs, through the same code as in execute,
and only for the installation the helper names."""

import logging
import os
import subprocess
import unittest

import scenarios
import support as S
from sekka_engine import canonical
from sekka_engine import contract as c

logging.disable(logging.CRITICAL)

INSTALLATION = "1f7d9c62-4a1b-4c8e-9d3f-2b6a8e0c5d47"
MUTATING = ("resizeContainer", "addPartition", "addVolume", "changeVolumeRole", "rename", "eraseVolume")


def changes(commands):
    """Every command in `commands` that could change a disk or the startup
    selection."""
    out = []
    for cmd in commands:
        if cmd[0] == "diskutil" and any(m in cmd for m in MUTATING) and "limits" not in cmd:
            out.append(cmd)
        elif cmd[0] in ("newfs_msdos", "hdiutil", "tar") or (cmd[0] == "bless" and "--setBoot" in cmd):
            out.append(cmd)
    return out


def messages(r):
    return r["channel"].messages()


def failure(r):
    [f] = [m["message"]["payload"] for m in messages(r) if m["message"]["type"] == "failure"]
    return f


def validate(r):
    validator = os.environ.get("SEKKA_TRANSCRIPT_VALIDATOR")
    if not validator:
        return None
    lines = b"".join(line + b"\n" for line in r["channel"].lines)
    plan = r["plan"]
    digest = canonical.digest_bytes(plan) if isinstance(plan, bytes) else canonical.digest(plan)
    p = subprocess.run([validator, "startup-disk", digest], input=lines, capture_output=True)
    return p.returncode, p.stdout.decode().strip()


class StartupDiskRetry(unittest.TestCase):
    def setUp(self):
        self.t = S.TempDir()

    def tearDown(self):
        self.t.cleanup()

    def failed_execute(self, before=scenarios.bless_fails):
        r = scenarios.execute(self.t, before=before)
        f = [m["message"]["payload"] for m in r["channel"].messages() if m["message"]["type"] == "failure"][0]
        self.assertEqual((f["stage"], r["outcome"]), ("startup-disk", "failed"))
        mac = r["mac"]
        self.assertEqual(mac.nvram_boot_group, mac.boot_group, "the failed bless left macOS selected")
        return r

    def stub_system(self, mac):
        stub = mac.ordered()[2]
        return [v for v in stub.container.volumes if "System" in v.roles][0]

    def test_a_failed_bless_is_retried_and_only_the_startup_disk_runs(self):
        for before in (scenarios.bless_fails, scenarios.bless_refuses_the_password):
            with self.subTest(before.__name__):
                self.t.cleanup()
                self.t = S.TempDir()
                r = self.failed_execute(before)
                execute_facts = [
                    [x["name"] for x in m["message"]["payload"]["facts"]]
                    for m in r["channel"].messages()
                    if m["message"]["type"] == "checkpoint" and m["message"]["payload"]["stage"] == "recovery"
                ]
                self.assertTrue(execute_facts)
                rr = scenarios.retry(r, self.t)
                self.assertEqual(rr["outcome"], "waiting-for-recovery")
                ms = messages(rr)
                self.assertEqual([m["message"]["type"] for m in ms], ["hello", "log", "intent", "checkpoint", "completion"])
                self.assertEqual(ms[0]["message"]["payload"]["mode"], "startup-disk")
                digest = canonical.digest(r["plan"])
                intent, checkpoint = ms[2]["message"]["payload"], ms[3]["message"]["payload"]
                self.assertEqual((intent["stage"], intent["plan"]), ("startup-disk", digest))
                self.assertEqual((checkpoint["stage"], checkpoint["plan"], checkpoint["verification"]), ("startup-disk", digest, "policy-read-back"))
                self.assertEqual(
                    [f["name"] for f in checkpoint["facts"]],
                    ["startup-volume-group", "bless-boot-device", "nvram-boot-volume-group", "recovery-app"],
                )
                # One bless, and nothing else that changes anything.
                self.assertEqual([cmd[:2] for cmd in changes(rr["commands"])], [["bless", "--setBoot"]])
                # Nothing changed before the acknowledgement.
                [ack] = [e for e in rr["channel"].events if e[0] == "ack"]
                self.assertEqual(ack[2], rr["state_before"])
                mac = rr["mac"]
                system = self.stub_system(mac)
                self.assertEqual(mac.nvram_boot_group, system.group)
                core = os.path.join(system.dir, "System/Library/CoreServices")
                self.assertTrue(os.path.exists(os.path.join(core, "SystemVersion-disabled.plist")))
                self.assertFalse(os.path.exists(os.path.join(core, "SystemVersion.plist")))
                self.assertTrue(os.path.exists(os.path.join(system.dir, ".IAPhysicalMedia")))
                self.assertNotIn(S.PASSWORD, b"\n".join(rr["channel"].lines))
                # The ESP was only looked at read-only, and nothing is left mounted.
                esp = mac.ordered()[3]
                self.assertIn((esp.uuid, "ro"), rr["mounts"])
                self.assertNotIn((esp.uuid, "rw"), rr["mounts"])
                stub_volumes = {v.uuid for v in mac.ordered()[2].container.volumes}
                self.assertLessEqual({u for u, mode in rr["mounts"] if mode == "rw"}, stub_volumes)
                self.assertEqual(mac.mounted_now(), [])
                result = validate(rr)
                if result is not None:
                    self.assertEqual(result, (0, "accepted: WaitingForRecovery"))

    def test_a_boot_entry_execute_accepted_is_accepted_again(self):
        """The image passes the partitions apart, the ESP first: execute fills
        and verifies the entry, and the retry checks it by the same rule, as
        first-boot setup reads the command line."""
        entry = (
            b"title Sekka\nlinux /EFI/nixos/kernel.efi\ninitrd /EFI/nixos/initrd.efi\n"
            b"options sekka.esp=PARTUUID=@SEKKA_ESP_PARTUUID@ init=/nix/store/x/init\n"
            b"options quiet sekka.root=PARTUUID=@SEKKA_ROOT_PARTUUID@\n"
        )
        r = scenarios.execute(self.t, before=scenarios.bless_fails, release_kw={"boot_entry": entry})
        esp = [m["message"]["payload"] for m in messages(r) if m["message"]["type"] == "checkpoint" and m["message"]["payload"]["stage"] == "esp"][0]
        facts = {f["name"]: f["value"] for f in esp["facts"]}
        root, esp_part = r["mac"].ordered()[4].uuid.lower(), r["mac"].ordered()[3].uuid.lower()
        self.assertEqual(facts["kernel-arguments"], f"sekka.esp=PARTUUID={esp_part} sekka.root=PARTUUID={root}")
        rr = scenarios.retry(r, self.t)
        self.assertEqual(rr["outcome"], "waiting-for-recovery", messages(rr)[-2]["message"]["payload"])

    def test_a_retry_that_fails_again_says_so(self):
        r = self.failed_execute()
        rr = scenarios.retry(r, self.t, bless_mode="fail")
        self.assertEqual(rr["outcome"], "failed")
        f = failure(rr)
        self.assertEqual((f["stage"], f["failure"]["code"], f["failure"]["disk_changed"]), ("startup-disk", "startup-disk-failed", "yes"))
        self.assertIn("0xe00002bc", f["failure"]["apple_diagnostic"])
        result = validate(rr)
        if result is not None:
            self.assertEqual(result, (0, "accepted: Failed"))

    def assert_refused(self, rr, code):
        self.assertEqual(rr["outcome"], "failed")
        types = [m["message"]["type"] for m in messages(rr)]
        self.assertNotIn("intent", types)
        self.assertEqual(types[-2:], ["failure", "completion"])
        f = failure(rr)
        self.assertEqual((f["stage"], f["failure"]["code"], f["failure"]["disk_changed"]), (None, code, "no"))
        self.assertEqual(changes(rr["commands"]), [])
        self.assertEqual(rr["mac"].state_digest(), rr["state_before"])
        result = validate(rr)
        if result is not None:
            self.assertEqual(result, (0, "accepted: Failed"))
        return f

    def test_another_installation_is_refused(self):
        r = self.failed_execute()
        f = self.assert_refused(scenarios.retry(r, self.t, installation="00000000-0000-4000-8000-000000000000"), "installation-not-found")
        self.assertIn("00000000-0000-4000-8000-000000000000", f["failure"]["message"])

    def test_a_mac_without_the_installation_is_refused(self):
        mac = S.j514s_clean(self.t.sub("mac"))
        asahi, host = S.load_asahi(mac)
        _, doc, _ = S.inspect(mac, asahi, host, self.t.sub("w0"))
        release = S.make_release(self.t.sub("assets"))
        plan = S.derive_plan(doc, release, 190 * S.GIB)
        r = {"mac": mac, "asahi": asahi, "host": host, "plan": plan}
        self.assert_refused(scenarios.retry(r, self.t), "installation-not-found")

    def test_without_an_acknowledgement_nothing_runs(self):
        r = self.failed_execute()
        channel = S.Channel(r["mac"], stop_at="startup-disk")
        rr = scenarios.retry(r, self.t, channel=channel)
        f = failure(rr)
        self.assertEqual((f["stage"], f["failure"]["code"]), ("startup-disk", "not-acknowledged"))
        self.assertEqual(changes(rr["commands"]), [])
        self.assertEqual(rr["mac"].state_digest(), rr["state_before"])
        self.assertEqual(rr["mac"].nvram_boot_group, rr["mac"].boot_group)

    def test_a_changed_installation_is_refused(self):
        def boot_object(mac):
            path = os.path.join(self.stub_system(mac).dir, c.RECOVERY_DATA_DIR, "boot.bin")
            with open(path, "ab") as f:
                f.write(b"tampered")

        def launch(mac):
            os.unlink(os.path.join(self.stub_system(mac).dir, ".IAPhysicalMedia"))

        def app(mac):
            os.unlink(os.path.join(self.stub_system(mac).dir, "Sekka Recovery.app", "Contents", "Info.plist"))

        def handoff(mac):
            esp = mac.ordered()[3]
            path = os.path.join(esp.dir, c.HANDOFF_PATH)
            with open(path, "rb") as f:
                doc = c.parse_handoff(f.read())
            doc["installation"] = "00000000-0000-4000-8000-000000000000"
            with open(path, "wb") as f:
                f.write(canonical.dumps(doc))

        def root_partition(mac):
            mac.parts.remove(mac.ordered()[4])

        def esp_stage2(mac):
            # What boot.bin chainloads: changed on the ESP after execute verified it.
            with open(os.path.join(mac.ordered()[3].dir, "m1n1", "boot.bin"), "ab") as f:
                f.write(b"planted")

        def esp_kernel(mac):
            os.unlink(os.path.join(mac.ordered()[3].dir, "EFI", "nixos", "kernel.efi"))

        def esp_extra(mac):
            with open(os.path.join(mac.ordered()[3].dir, "EFI", "BOOT", "extra.efi"), "wb") as f:
                f.write(b"an extra loader")

        for change in (boot_object, launch, app, handoff, root_partition, esp_stage2, esp_kernel, esp_extra):
            with self.subTest(change.__name__):
                self.t.cleanup()
                self.t = S.TempDir()
                r = self.failed_execute()
                change(r["mac"])
                self.assert_refused(scenarios.retry(r, self.t), "installation-changed")

    def test_the_request_is_checked_first(self):
        r = self.failed_execute()
        other = dict(r["plan"], machine=dict(r["plan"]["machine"], binding=c.machine_binding("00000000-0000-4000-8000-000000000000")))
        cases = (
            ({"plan": other}, "wrong-machine"),
            ({"plan": b'{"schema": "sekka.mac.plan/1"}'}, "plan-invalid"),
            ({"installation": INSTALLATION.upper()}, "installation-invalid"),
            ({"esp_inventory": "sha256:" + "0" * 64}, "installation-changed"),
            ({"esp_inventory": "not a digest"}, "esp-inventory-invalid"),
        ) + tuple(({"owner": owner, "password": password}, code) for owner, password, code in S.UNUSABLE_OWNERS)
        for i, (kw, code) in enumerate(cases):
            with self.subTest(code=code):
                rr = scenarios.retry(r, self.t, name=f"work-retry-{i}", **kw)
                self.assert_refused(rr, code)


if __name__ == "__main__":
    unittest.main()
