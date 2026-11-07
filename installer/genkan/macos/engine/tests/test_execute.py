# SPDX-License-Identifier: GPL-3.0-or-later
"""execute: one plan carried out on a fake Mac, stage by stage, and every
way the engine refuses or stops."""

import errno
import io
import json
import logging
import os
import plistlib
import signal
import subprocess
import sys
import textwrap
import time
import unittest

import scenarios
import support as S
from sekka_engine import canonical
from sekka_engine import contract as c
from sekka_engine.asahi import View
from sekka_engine.engine import STOP_LIMIT, Terminated

logging.disable(logging.CRITICAL)

MUTATING = ("resizeContainer", "addPartition", "addVolume", "changeVolumeRole", "rename", "eraseVolume")


def mutations(mac):
    out = []
    for cmd in mac.commands:
        if cmd[0] == "diskutil" and any(m in cmd for m in MUTATING) and "limits" not in cmd:
            out.append(cmd)
        elif cmd[0] in ("newfs_msdos",) or (cmd[0] == "bless" and "--setBoot" in cmd):
            out.append(cmd)
    return out


class Run:
    def __init__(self, r):
        self.__dict__.update(r)
        self.messages = r["channel"].messages()

    def types(self):
        return [m["message"]["type"] for m in self.messages]

    def payloads(self, kind):
        return [m["message"]["payload"] for m in self.messages if m["message"]["type"] == kind]

    def failure(self):
        [f] = self.payloads("failure")
        return f

    def facts(self, stage):
        [cp] = [p for p in self.payloads("checkpoint") if p["stage"] == stage]
        return {f["name"]: f["value"] for f in cp["facts"]}, cp["verification"]


class ExecuteTests(unittest.TestCase):
    def setUp(self):
        self.t = S.TempDir()

    def tearDown(self):
        self.t.cleanup()

    def run_plan(self, **kw):
        return Run(scenarios.execute(self.t, **kw))

    def assert_refused(self, run, code):
        self.assertEqual(run.outcome, "failed")
        self.assertEqual(run.types()[-2:], ["failure", "completion"])
        self.assertNotIn("intent", run.types())
        failure = run.failure()
        self.assertEqual((failure["stage"], failure["failure"]["code"], failure["failure"]["disk_changed"]), (None, code, "no"))
        self.assertEqual(mutations(run.mac), [])
        self.assertEqual(run.mac.state_digest(), run.state_before)
        return failure

    # The whole way ----------------------------------------------------------------

    def test_the_happy_path(self):
        run = self.run_plan()
        self.assertEqual(run.outcome, "waiting-for-recovery")
        intents = [p["stage"] for p in run.payloads("intent")]
        self.assertEqual(intents, c.macos_stages(True))
        self.assertEqual([p["stage"] for p in run.payloads("checkpoint")], intents)
        digest = canonical.digest(run.plan)
        self.assertTrue(all(p["plan"] == digest for p in run.payloads("intent") + run.payloads("checkpoint")))
        levels = {p["stage"]: p["verification"] for p in run.payloads("checkpoint")}
        self.assertEqual(
            levels,
            {
                "apple-inputs": "inspected",
                "shrink": "inspected",
                "stub-partition": "inspected",
                "stub-system": "inspected",
                "partitions": "inspected",
                "firmware": "read-back",
                "esp": "read-back",
                "root": "read-back",
                "recovery": "read-back",
                "startup-disk": "policy-read-back",
            },
        )
        mac = run.mac
        parts = mac.ordered()
        self.assertEqual([p.content for p in parts], ["Apple_APFS_ISC", "Apple_APFS", "Apple_APFS", "EFI", "Linux Filesystem", "Apple_APFS_Recovery"])
        macos, stub, esp, root = parts[1:5]
        a = run.plan["allocation"]
        self.assertEqual((macos.size, stub.size, esp.size, root.size), (a["macos_container_after"], a["stub"], a["esp"], a["root"]))
        self.assertEqual(stub.offset, run.plan["target"]["offset"])
        self.assertEqual(esp.offset, stub.offset + stub.size)
        self.assertEqual(root.offset, esp.offset + esp.size)
        self.assertEqual(esp.volume_name, "EFI - SEKKA")
        # The facts name what is really there.
        facts, _ = run.facts("partitions")
        self.assertEqual((facts["esp-partition"], facts["root-partition"]), (esp.uuid.lower(), root.uuid.lower()))
        facts, _ = run.facts("stub-partition")
        self.assertEqual(facts["stub-partition"], stub.uuid.lower())
        # The ESP: the payload's tree, the filled entry, the handoff, firmware.
        with open(os.path.join(esp.dir, S.BOOT_ENTRY)) as f:
            entry = f.read()
        self.assertIn(c.kernel_arguments(root.uuid.lower(), esp.uuid.lower()), entry)
        self.assertNotIn("@SEKKA_", entry)
        with open(os.path.join(esp.dir, c.HANDOFF_PATH), "rb") as f:
            handoff = c.parse_handoff(f.read())
        self.assertEqual((handoff["root"], handoff["esp"]), (root.uuid.lower(), esp.uuid.lower()))
        self.assertEqual(
            handoff["hints"],
            {"locale": "en_US.UTF-8", "keyboard": "us", "timezone": "America/Chicago", "computer_name": "Chris's MacBook Pro", "full_name": "Chris Allen"},
        )
        self.assertEqual(handoff["created"], c.format_timestamp(S.NOW))
        for rel, data in run.release["esp"].items():
            if rel != S.BOOT_ENTRY:
                with open(os.path.join(esp.dir, rel), "rb") as f:
                    self.assertEqual(f.read(), data, rel)
        self.assertTrue(os.path.isfile(os.path.join(esp.dir, "vendorfw", "firmware.cpio")))
        self.assertTrue(os.path.isfile(os.path.join(esp.dir, "asahi", "stub_info.json")))
        # The root partition holds the image, byte for byte.
        with open(mac.raw_path(root), "rb") as f:
            self.assertEqual(f.read(), run.release["root_image"])
        facts, _ = run.facts("root")
        self.assertEqual(facts["root-read-back"], run.release["root_digest"])
        # The stub: Sekka's Recovery app and its data, and none of Asahi's step 2.
        system = [v for v in stub.container.volumes if "System" in v.roles][0]
        self.assertFalse(os.path.exists(os.path.join(system.dir, "Finish Installation.app")))
        self.assertTrue(os.path.isfile(os.path.join(system.dir, "Sekka Recovery.app", "Contents", "MacOS", "Sekka Recovery")))
        with open(os.path.join(system.dir, ".IAPhysicalMedia"), "rb") as f:
            self.assertEqual(plistlib.load(f)["AppName"], "Sekka Recovery.app")
        with open(os.path.join(system.dir, c.RECOVERY_DATA_DIR, "recovery.json"), "rb") as f:
            config = c.parse_recovery_config(f.read())
        self.assertEqual(config["installation"], "1f7d9c62-4a1b-4c8e-9d3f-2b6a8e0c5d47")
        self.assertEqual((config["esp"], config["root"]), (esp.uuid.lower(), root.uuid.lower()))
        self.assertEqual(config["stub"]["volume_group"], system.group.lower())
        self.assertEqual(config["owner"], {"short_name": S.OWNER, "full_name": "Chris Allen", "macos_volume_group": mac.boot_group.lower()})
        with open(os.path.join(system.dir, c.RECOVERY_DATA_DIR, "boot.bin"), "rb") as f:
            boot = f.read()
        with open(os.path.join(S.asahi_root(), "boot", "m1n1.bin"), "rb") as f:
            m1n1 = f.read()
        self.assertTrue(boot.startswith(m1n1))
        self.assertEqual(
            boot[len(m1n1) :],
            f"chosen.asahi,efi-system-partition={esp.uuid.lower()}\nchainload={esp.uuid.lower()};m1n1/boot.bin\n".encode() + b"\0\0\0\0",
        )
        self.assertTrue(os.path.exists(os.path.join(system.dir, "System/Library/CoreServices/SystemVersion-disabled.plist")))
        self.assertFalse(os.path.exists(os.path.join(system.dir, "System/Library/CoreServices/SystemVersion.plist")))
        self.assertEqual(mac.nvram_boot_group, system.group)
        # The owner's password never appears in what the engine says.
        transcript = b"\n".join(run.channel.lines)
        self.assertNotIn(S.PASSWORD, transcript)
        self.assertNotIn(S.PASSWORD.decode(), json.dumps([cmd for cmd in mac.commands]))

    def test_only_sekkas_own_volumes_are_mounted_writable_and_nothing_stays_mounted(self):
        run = self.run_plan()
        self.assertEqual(run.outcome, "waiting-for-recovery")
        mac = run.mac
        stub, esp = mac.ordered()[2], mac.ordered()[3]
        own = {v.uuid for v in stub.container.volumes} | {esp.uuid}
        writable = {uuid for uuid, mode in mac.mount_log if mode == "rw"}
        self.assertTrue(writable)
        self.assertLessEqual(writable, own, "only the stub's volumes and the ESP are mounted read-write")
        self.assertIn(esp.uuid, writable)
        self.assertEqual(mac.mounted_now(), [], "the run unmounts everything it mounted, the verified ESP included")
        # macOS's own mounts are left alone (the fake refuses to unmount them).
        self.assertTrue(any(v.mounted == "system" for ct in mac.containers.values() for v in ct.volumes))

    def test_unreadable_preferences_give_no_hint_and_never_stall(self):
        """The owner's account can put anything at its preference paths: a
        FIFO, a link to a device, a huge file. The engine reads them before
        any intent, without following links or waiting, and takes no hint."""

        def before(mac, host, release):
            prefs = mac.fs(f"/Users/{S.OWNER}/Library/Preferences")
            global_prefs = os.path.join(prefs, ".GlobalPreferences.plist")
            os.unlink(global_prefs)
            os.mkfifo(global_prefs)
            toolbox = os.path.join(prefs, "com.apple.HIToolbox.plist")
            os.unlink(toolbox)
            os.symlink("/dev/zero", toolbox)

        run = self.run_plan(before=before)
        self.assertEqual(run.outcome, "waiting-for-recovery")
        esp = run.mac.ordered()[3]
        with open(os.path.join(esp.dir, c.HANDOFF_PATH), "rb") as f:
            hints = c.parse_handoff(f.read())["hints"]
        self.assertEqual((hints["locale"], hints["keyboard"]), (None, None))
        self.assertEqual(hints["timezone"], "America/Chicago")

    def test_preferences_another_account_owns_give_no_hint(self):
        def before(mac, host, release):
            host.owner_uid = os.getuid() + 1

        run = self.run_plan(before=before)
        esp = run.mac.ordered()[3]
        with open(os.path.join(esp.dir, c.HANDOFF_PATH), "rb") as f:
            hints = c.parse_handoff(f.read())["hints"]
        self.assertEqual((hints["locale"], hints["keyboard"]), (None, None))

    def test_nothing_changes_before_an_acknowledgement(self):
        run = self.run_plan()
        events = run.channel.events
        last = run.state_before
        for kind, stage, state in events:
            if kind == "ack":
                self.assertEqual(state, last, f"the disk changed before {stage} was acknowledged")
            last = state
        acks = [e for e in events if e[0] == "ack"]
        checkpoints = {e[1]: e[2] for e in events if e[0] == "checkpoint"}
        # Fetching Apple's files changes no disk.
        self.assertEqual(checkpoints["apple-inputs"], run.state_before)
        self.assertEqual(len(acks), len(c.macos_stages(True)))

    def test_without_acknowledgement_a_stage_never_starts(self):
        for stage, changed in (("apple-inputs", "no"), ("shrink", "no"), ("partitions", "yes"), ("startup-disk", "yes")):
            with self.subTest(stage=stage):
                self.t.cleanup()
                self.t = S.TempDir()
                mac = S.j514s_clean(self.t.sub("mac"))
                asahi, host = S.load_asahi(mac)
                _, doc, _ = S.inspect(mac, asahi, host, self.t.sub("w0"))
                release = S.make_release(self.t.sub("assets"))
                plan = S.derive_plan(doc, release, 190 * S.GIB)
                channel = S.Channel(mac, stop_at=stage)
                outcome, channel = S.run_engine("execute", mac, asahi, host, self.t.sub("w"), plan=plan, release=release, channel=channel)
                self.assertEqual(outcome, "failed")
                messages = channel.messages()
                failure = messages[-2]["message"]["payload"]
                self.assertEqual((failure["stage"], failure["failure"]["code"], failure["failure"]["disk_changed"]), (stage, "not-acknowledged", changed))
                acked = [e for e in channel.events if e[0] == "ack"]
                self.assertEqual(acked[-1][1], stage)
                self.assertEqual(channel.events[-1][2], acked[-1][2], "nothing changed after the refused acknowledgement")

    def test_free_space_needs_no_shrink(self):
        run = self.run_plan(builder=S.j514s_free_space, place="free-space", sekka=150 * S.GIB)
        self.assertEqual(run.outcome, "waiting-for-recovery")
        self.assertEqual([p["stage"] for p in run.payloads("intent")], c.macos_stages(False))
        self.assertFalse(any("resizeContainer" in cmd and "limits" not in cmd for cmd in run.mac.commands))

    # Refusals before anything changes ------------------------------------------------

    def test_a_stale_plan_is_refused(self):
        run = self.run_plan(before=scenarios.layout_changes)
        self.assert_refused(run, "layout-changed")

    def test_a_plan_for_another_mac_is_refused(self):
        def edit(plan):
            plan["machine"]["binding"] = c.machine_binding("00000000-0000-4000-8000-000000000000")

        self.assert_refused(self.run_plan(plan_edit=edit), "wrong-machine")

    def test_a_plan_for_another_disk_is_refused(self):
        def edit(plan):
            plan["disk"]["disk"] = "00000000-0000-4000-8000-000000000000"

        self.assert_refused(self.run_plan(plan_edit=edit), "wrong-disk")

    def test_a_changed_asset_is_refused(self):
        def before(mac, host, release):
            path = os.path.join(release["dir"], "payload.zip")
            with open(path, "r+b") as f:
                f.seek(100)
                f.write(b"tampered")

        failure = self.assert_refused(self.run_plan(before=before), "asset-mismatch")
        self.assertIn("payload.zip", failure["failure"]["message"])

    def test_a_linked_asset_is_refused(self):
        def before(mac, host, release):
            path = os.path.join(release["dir"], "manifest.json")
            os.rename(path, path + ".real")
            os.symlink(path + ".real", path)

        self.assert_refused(self.run_plan(before=before), "asset-mismatch")

    def test_an_unsupported_board_is_refused(self):
        mac = S.m1_pro_with_asahi(self.t.sub("mac"))
        asahi, host = S.load_asahi(mac)
        _, doc, _ = S.inspect(mac, asahi, host, self.t.sub("w0"))
        release = S.make_release(self.t.sub("assets"))
        plan = S.derive_plan(doc, release, 100 * S.GIB)
        before = mac.state_digest()
        outcome, channel = S.run_engine("execute", mac, asahi, host, self.t.sub("w"), plan=plan, release=release)
        failure = channel.messages()[-2]["message"]["payload"]
        self.assertEqual((outcome, failure["stage"], failure["failure"]["code"]), ("failed", None, "not-admitted"))
        self.assertEqual(mac.state_digest(), before)

    def test_untested_firmware_is_refused(self):
        mac = S.j514s_clean(self.t.sub("mac"))
        mac.sys_firmware = "mBoot-20458.1.1"
        asahi, host = S.load_asahi(mac)
        _, doc, _ = S.inspect(mac, asahi, host, self.t.sub("w0"))
        release = S.make_release(self.t.sub("assets"))
        plan = S.derive_plan(doc, release, 190 * S.GIB)
        outcome, channel = S.run_engine("execute", mac, asahi, host, self.t.sub("w"), plan=plan, release=release)
        failure = channel.messages()[-2]["message"]["payload"]["failure"]
        self.assertEqual((outcome, failure["code"], failure["disk_changed"]), ("failed", "not-admitted", "no"))
        self.assertEqual(mutations(mac), [])

    def test_a_plan_that_isnt_canonical_or_consistent_is_refused(self):
        def spaced(plan):
            plan["volume_name"] = "Sekka"

        mac = S.j514s_clean(self.t.sub("mac"))
        asahi, host = S.load_asahi(mac)
        _, doc, _ = S.inspect(mac, asahi, host, self.t.sub("w0"))
        release = S.make_release(self.t.sub("assets"))
        plan = S.derive_plan(doc, release, 190 * S.GIB)
        for name, data in (
            ("pretty", json.dumps(plan, indent=1).encode()),
            ("arithmetic", canonical.dumps(dict(plan, allocation=dict(plan["allocation"], root=plan["allocation"]["root"] + S.MIB)))),
            ("unknown field", canonical.dumps(dict(plan, extra=True))),
            ("schema", canonical.dumps(dict(plan, schema="sekka.mac.plan/2"))),
        ):
            with self.subTest(name):
                outcome, channel = S.run_engine("execute", mac, asahi, host, self.t.sub("w-" + name.replace(" ", "-")), plan=data, release=release)
                failure = channel.messages()[-2]["message"]["payload"]["failure"]
                self.assertEqual(failure["code"], "plan-invalid")
        # Consistent in itself, but not where Asahi sees the container end.
        moved = dict(plan, target={"offset": plan["target"]["offset"] + S.MIB, "size": plan["target"]["size"]})
        outcome, channel = S.run_engine("execute", mac, asahi, host, self.t.sub("w-moved"), plan=moved, release=release)
        self.assertEqual(channel.messages()[-2]["message"]["payload"]["failure"]["code"], "plan-mismatch")

    def test_a_shrink_past_asahis_minimum_is_refused(self):
        def edit(plan):
            shrink = plan["actions"][0]
            delta = 120 * S.GIB
            shrink["to"] -= delta
            plan["allocation"]["macos_container_after"] -= delta
            plan["target"]["offset"] -= delta
            plan["target"]["size"] += delta
            plan["allocation"]["sekka"] += delta
            plan["allocation"]["root"] += delta
            plan["actions"][-1]["size"] += delta

        self.assert_refused(self.run_plan(plan_edit=edit), "shrink-too-far")

    def test_a_plan_for_another_profile_is_refused(self):
        def edit(plan):
            plan["release"]["profile_digest"] = canonical.digest_bytes(b"another profile")

        self.assert_refused(self.run_plan(plan_edit=edit), "profile-mismatch")

    def test_the_password_and_owner_are_checked_before_anything(self):
        cases = [({"owner": owner, "password": password}, code) for owner, password, code in S.UNUSABLE_OWNERS]
        for kw, code in cases + [({"installation": "NOT-A-UUID"}, "installation-invalid")]:
            with self.subTest(code=code, kw=kw):
                self.t.cleanup()
                self.t = S.TempDir()
                self.assert_refused(self.run_plan(run_kw=kw), code)

    def test_a_plan_beside_a_developer_install_is_refused(self):
        """A plan for the free space beside Sekka's developer image, which
        the inspection described but stopped on: execute's own inspection
        refuses it, and nothing changes."""
        run = self.run_plan(builder=lambda root: S.j514s_developer(root, free_space=True), place="free-space", sekka=60 * S.GIB)
        failure = self.assert_refused(run, "blocked")
        self.assertIn("developer installation of Sekka", failure["failure"]["message"])

    def test_blocking_issues_are_refused(self):
        def before(mac, host, release):
            mac.pmset = "Now drawing from 'Battery Power'\n -InternalBattery-0 (id=1)\t55%; discharging\n"

        failure = self.assert_refused(self.run_plan(before=before), "blocked")
        self.assertIn("charger", failure["failure"]["message"])

    # Failures part way -----------------------------------------------------------------

    def test_a_resize_failure_keeps_apples_words_and_changes_nothing(self):
        run = self.run_plan(before=scenarios.resize_fails)
        self.assertEqual(run.outcome, "failed")
        failure = run.failure()
        self.assertEqual((failure["stage"], failure["failure"]["code"], failure["failure"]["disk_changed"]), ("shrink", "resize-failed", "no"))
        self.assertIn("Error: -69606: A problem occurred while resizing APFS Container structures", failure["failure"]["apple_diagnostic"])
        self.assertIn("exited with status 1", failure["failure"]["apple_diagnostic"])
        self.assertEqual(run.mac.state_digest(), run.state_before)
        resizes = [cmd for cmd in run.mac.commands if "resizeContainer" in cmd and "limits" not in cmd]
        self.assertEqual(len(resizes), 1, "the resize is never retried")
        self.assertEqual([p["stage"] for p in run.payloads("intent")], ["apple-inputs", "shrink"])

    def test_a_resize_that_fails_part_way_reports_the_change(self):
        def before(mac, host, release):
            original = mac.resize

            def partial(args):
                if args[1:] != ["limits", "-plist"]:
                    p = mac.part_by_name(args[0])
                    p.size -= S.GIB
                    p.container.ceiling -= S.GIB
                    return 1, b"Error: -69519: The target disk is too small\n", b""
                return original(args)

            mac.resize = partial

        run = self.run_plan(before=before)
        failure = run.failure()
        self.assertEqual((failure["stage"], failure["failure"]["disk_changed"]), ("shrink", "yes"))

    def test_a_root_read_back_mismatch_stops_the_installation(self):
        run = self.run_plan(before=scenarios.read_back_corrupts)
        failure = run.failure()
        self.assertEqual((failure["stage"], failure["failure"]["code"], failure["failure"]["disk_changed"]), ("root", "root-read-back-mismatch", "yes"))
        self.assertNotIn("recovery", [p["stage"] for p in run.payloads("intent")])

    def test_apple_inputs_failing_changes_nothing(self):
        mac = S.j514s_clean(self.t.sub("mac"))
        asahi, host = S.load_asahi(mac)
        _, doc, _ = S.inspect(mac, asahi, host, self.t.sub("w0"))
        release = S.make_release(self.t.sub("assets"))
        plan = S.derive_plan(doc, release, 190 * S.GIB)
        before = mac.state_digest()
        outcome, channel = S.run_engine("execute", mac, asahi, host, self.t.sub("w"), plan=plan, release=release, restore=b"not a zip at all" * 100)
        failure = channel.messages()[-2]["message"]["payload"]
        self.assertEqual((failure["stage"], failure["failure"]["code"], failure["failure"]["disk_changed"]), ("apple-inputs", "apple-inputs-failed", "no"))
        self.assertEqual(mac.state_digest(), before)

    # Asked to stop ---------------------------------------------------------------

    def fresh(self):
        """A new folder for the fake Mac and its files, for each subtest."""
        self.t = S.TempDir()
        self.addCleanup(self.t.cleanup)

    def stopped(self, channel=S.Channel, before=None, mode="execute", stops=Terminated, **kw):
        """Execute (or inspect) in a working folder of the engine's own, as
        a real run does, until SIGTERM (or `stops`) stops it."""
        mac = S.j514s_clean(self.t.sub("mac"))
        asahi, host = S.load_asahi(mac)
        _, doc, _ = S.inspect(mac, asahi, host, self.t.sub("w0"))
        release = S.make_release(self.t.sub("assets"))
        plan = S.derive_plan(doc, release, 190 * S.GIB)
        if before:
            before(mac)
        state = mac.state_digest()
        channel = channel(mac)
        with self.assertRaises(stops):
            S.run_engine(mode, mac, asahi, host, None, plan=plan, release=release, channel=channel, **kw)
        return Run({"mac": mac, "channel": channel, "plan": plan, "state_before": state})

    def test_a_cancel_while_apples_files_download_cleans_up(self):
        """The helper cancels with SIGTERM while Apple's files download: the
        engine stops there, deletes its working folder with Apple's files in
        it, leaves nothing mounted, and says nothing more."""

        class Cancelling(S.Channel):
            sent = False

            def write(self, data):
                n = super().write(data)
                message = json.loads(self.lines[-1])["message"]
                if not self.sent and message["type"] == "progress" and message["payload"]["stage"] == "apple-inputs":
                    self.sent = True
                    os.kill(os.getpid(), signal.SIGTERM)
                return n

        run = self.stopped(Cancelling)
        workdir = run.channel.engine.workdir
        self.assertTrue(workdir and not os.path.exists(workdir), workdir)
        self.assertEqual(run.types()[-1], "progress")
        self.assertNotIn("failure", run.types())
        self.assertEqual(run.mac.mounted_now(), [])
        self.assertEqual(run.mac.state_digest(), run.state_before)
        # The cleanup had a time limit (test_a_cleanup_that_never_ends_is_ended).
        self.assertTrue(run.channel.stop_limit and run.channel.stop_limit <= STOP_LIMIT, run.channel.stop_limit)

    def test_a_stop_asked_while_the_disk_changes_waits_for_the_stage(self):
        """SIGTERM while the container is being resized interrupts nothing:
        the resize finishes and is recorded, and no other stage starts."""

        def before(mac):
            original = mac.resize

            def resize(args):
                if args[1:] != ["limits", "-plist"]:
                    os.kill(os.getpid(), signal.SIGTERM)
                return original(args)

            mac.resize = resize

        run = self.stopped(before=before)
        self.assertEqual([p["stage"] for p in run.payloads("intent")], ["apple-inputs", "shrink"])
        self.assertEqual([p["stage"] for p in run.payloads("checkpoint")], ["apple-inputs", "shrink"])
        self.assertEqual(run.mac.ordered()[1].size, run.plan["actions"][0]["to"])
        self.assertEqual(run.mac.mounted_now(), [])
        self.assertFalse(os.path.exists(run.channel.engine.workdir))
        self.assertTrue(run.channel.stop_limit and run.channel.stop_limit <= STOP_LIMIT, run.channel.stop_limit)

    def test_a_cleanup_that_never_ends_is_ended(self):
        """A cancel whose cleanup is stuck on a tool that never answers (the
        diskutil that unmounts, held up by what held the run up) still ends
        the engine, STOP_LIMIT seconds after it was asked, by a signal
        Python doesn't handle; the helper then reads the end of the
        transcript. Here the limit is a second."""
        code = textwrap.dedent(
            f"""
            import os, signal, subprocess, sys
            sys.path.insert(0, {S.ENGINE!r})
            from sekka_engine import engine as E
            E.STOP_LIMIT = 1
            e = E.Engine(host=None, asahi=None, profile_bytes=b"", out=None, adapter="", privileged=False)
            e.handle_sigterm()
            try:
                os.kill(os.getpid(), signal.SIGTERM)
            finally:
                # As MacHost runs a tool, its output captured.
                tool = subprocess.Popen(
                    [sys.executable, "-c", "import time; time.sleep(60)"],
                    stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.PIPE, close_fds=True,
                )
                print(tool.pid, flush=True)
                tool.communicate()
            """
        )
        started = time.monotonic()
        p = subprocess.run([sys.executable, "-I", "-c", code], stdin=subprocess.DEVNULL, capture_output=True, timeout=60)
        took = time.monotonic() - started
        try:
            self.assertEqual(p.returncode, -signal.SIGALRM, p.stderr.decode()[-2000:])
            self.assertLess(took, 20)
        finally:
            for pid in p.stdout.split():
                try:
                    os.kill(int(pid), signal.SIGKILL)
                except ProcessLookupError:
                    pass

    def test_a_cancel_while_the_mac_is_looked_at_unmounts_what_it_mounted(self):
        """SIGTERM while a view of the Mac is still being built, as its
        first volume is mounted (the mount may finish though diskutil was
        stopped): what it mounted is unmounted again, in an inspection and
        in execute's own look before its first stage."""

        def building():
            frame = sys._getframe()
            while frame is not None:
                if frame.f_code.co_name == "__init__" and isinstance(frame.f_locals.get("self"), View):
                    return frame.f_locals["self"]
                frame = frame.f_back
            return None

        def before(nth):
            def change(mac):
                original = mac.cmd_diskutil
                views = []

                def diskutil(args, input):
                    out = original(args, input)
                    view = building()
                    if args[0] == "mount" and view is not None and view not in views:
                        views.append(view)
                        if len(views) == nth:
                            os.kill(os.getpid(), signal.SIGTERM)
                    return out

                mac.cmd_diskutil = diskutil

            return change

        # Execute looks at the Mac, then opens the view its stages work
        # through.
        for mode, nth in (("inspect", 1), ("execute", 1), ("execute", 2)):
            with self.subTest(mode=mode, view=nth):
                self.fresh()
                run = self.stopped(before=before(nth), mode=mode)
                self.assertTrue(run.mac.mount_log)
                self.assertEqual(run.mac.mounted_now(), [])
                self.assertNotIn("failure", run.types())
                self.assertNotIn("intent", run.types())
                self.assertEqual(run.mac.state_digest(), run.state_before)

    def test_a_stage_that_changes_the_disk_finishes_when_the_helper_goes_away(self):
        """The helper stops while Sekka's startup volume is set up (Asahi
        prints its progress to the diagnostic log) or while Sekka is written
        (progress goes in the transcript): nothing reads either any more,
        and neither stops the stage. It ends verified, and the checkpoint
        the engine then can't write stops the run before another starts."""

        class Gone(S.Channel):
            """The helper, until it has acknowledged `at`. After that what
            the engine writes is kept here for the test, and raises."""

            at = None

            def __init__(self, mac):
                super().__init__(mac)
                self.gone = False
                self.lost = []

            def read(self, n):
                out = super().read(n)
                stage = json.loads(self.lines[-1])["message"]["payload"]["stage"]
                if stage == self.at and not self.pending:
                    self.gone = True
                return out

            def write(self, data):
                if not self.gone:
                    return super().write(data)
                self.lost += [json.loads(line)["message"] for line in data.splitlines()]
                raise BrokenPipeError(errno.EPIPE, "Broken pipe")

        class Log(io.StringIO):
            """Standard error, read by the helper until it goes."""

            channel = None
            dropped = 0

            def write(self, text):
                if self.channel is not None and self.channel.gone:
                    self.dropped += 1
                    raise BrokenPipeError(errno.EPIPE, "Broken pipe")
                return super().write(text)

        for stage, verification in (("stub-system", "inspected"), ("root", "read-back")):
            with self.subTest(stage=stage):
                self.fresh()
                log = Log()

                class GoneAt(Gone):
                    at = stage

                    def __init__(self, mac):
                        super().__init__(mac)
                        log.channel = self

                run = self.stopped(GoneAt, stops=BrokenPipeError, printing=log)
                self.assertEqual(run.payloads("intent")[-1]["stage"], stage)
                lost = [m for m in run.channel.lost if m["type"] != "progress"]
                self.assertEqual([(m["type"], m["payload"]["stage"]) for m in lost], [("checkpoint", stage)])
                self.assertEqual(lost[0]["payload"]["verification"], verification)
                if stage == "root":
                    self.assertIn("progress", [m["type"] for m in run.channel.lost])
                else:
                    self.assertGreater(log.dropped, 0)
                self.assertEqual(run.mac.mounted_now(), [])

    def test_apple_inputs_fetch_only_what_the_stub_needs(self):
        run = self.run_plan()
        spool = run.channel.engine.restore["spool"]
        restore = S.make_restore_zip()
        import io
        import zipfile

        zf = zipfile.ZipFile(io.BytesIO(restore))
        for name in ("AssetData/boot/Firmware/all_flash/iBoot.j314s.RELEASE.im4p", "AssetData/payloadv2/payload.000"):
            info = zf.getinfo(name)
            self.assertFalse(spool.covered(info.header_offset + 100, info.header_offset + 200), name)
        facts, level = run.facts("apple-inputs")
        self.assertLess(int(facts["fetched-bytes"]), len(restore) // 2)
        self.assertIn("No digest of Apple's bytes is checked", facts["authenticity"])

    def test_startup_disk_is_read_back(self):
        # bless failing outright, and bless exiting 0 having refused the
        # password (the fake Mac's owner has another one).
        for mode, password, code in (("fail", S.PASSWORD, "startup-disk-failed"), ("ok", b"another password", "startup-disk-not-selected")):
            with self.subTest(code=code):
                self.t.cleanup()
                self.t = S.TempDir()

                def before(mac, host, release, mode=mode, password=password):
                    mac.bless_mode = mode
                    mac.owner_password = password

                run = self.run_plan(before=before)
                failure = run.failure()
                self.assertEqual((failure["stage"], failure["failure"]["code"], failure["failure"]["disk_changed"]), ("startup-disk", code, "yes"))
                self.assertNotEqual(run.mac.nvram_boot_group, None)
                self.assertEqual(run.mac.nvram_boot_group, run.mac.boot_group)


if __name__ == "__main__":
    unittest.main()
