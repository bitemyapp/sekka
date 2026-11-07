# SPDX-License-Identifier: GPL-3.0-or-later
"""Apple's files take room in the macOS container while Sekka installs: the
apple-inputs stage keeps them in the engine's workdir (on a Mac, under
/private/tmp on macOS's own volume) until the run ends. Here the fake Mac
charges that folder to the macOS container's free space, as a Mac would."""

import logging
import os
import re
import unittest

import support as S
from sekka_engine import contract as c

logging.disable(logging.CRITICAL)

# Big enough that Apple's members don't fit in the one MiB a plan at the
# maximum size leaves above Asahi's minimum.
BASESYSTEM = 6 * S.MIB


class GrowsAtShrink(S.Channel):
    """The helper's side, which also leaves more of the engine's own files
    in its workdir just before the shrink is acknowledged."""

    def __init__(self, mac, work, extra):
        super().__init__(mac)
        self.work, self.extra = work, extra

    def read(self, n):
        if not self.pending and self.extra:
            message = S.json.loads(self.lines[-1])["message"]
            if message["payload"].get("stage") == "shrink":
                with open(os.path.join(self.work, "extra"), "wb") as f:
                    f.write(b"\1" * self.extra)
                self.extra = 0
        return super().read(n)


def max_sekka(doc, staging):
    """plan.rs capacity() for the shrink candidate."""
    cand = [k for k in doc["candidates"] if k["kind"] == "shrink-container"][0]
    raw = max(0, cand["available"] - staging) + cand["adjacent_free"]
    return raw // c.ALIGN * c.ALIGN - c.ALIGN


class AppleInputsRoom(unittest.TestCase):
    def setUp(self):
        self.t = S.TempDir()

    def tearDown(self):
        self.t.cleanup()

    def run_at(self, staging, extra=0):
        mac = S.j514s_clean(self.t.sub("mac"))
        work = self.t.sub("work")
        mac.charge = work
        asahi, host = S.load_asahi(mac)
        _, doc, _ = S.inspect(mac, asahi, host, self.t.sub("w0"))
        release = S.make_release(self.t.sub("assets"))
        plan = S.derive_plan(doc, release, max_sekka(doc, staging))
        before = mac.state_digest()
        outcome, channel = S.run_engine(
            "execute",
            mac,
            asahi,
            host,
            work,
            plan=plan,
            release=release,
            restore=S.make_restore_zip(BASESYSTEM),
            channel=GrowsAtShrink(mac, work, extra),
        )
        return mac, doc, plan, outcome, channel, before

    def test_a_plan_without_room_for_apples_files_stops_before_any_change(self):
        mac, doc, plan, outcome, channel, before = self.run_at(staging=0)
        self.assertEqual(outcome, "failed")
        failure = channel.messages()[-2]["message"]["payload"]
        self.assertEqual(
            (failure["stage"], failure["failure"]["code"], failure["failure"]["disk_changed"]),
            ("apple-inputs", "apple-inputs-no-room", "no"),
        )
        self.assertIn("check this Mac again", failure["failure"]["message"])
        self.assertEqual(mac.state_digest(), before)
        self.assertFalse(any("resizeContainer" in cmd and "limits" not in cmd for cmd in mac.commands))
        self.assertFalse(os.path.exists(os.path.join(self.t.sub("work"), "restore.spool")), "nothing was fetched")

    def test_the_engines_own_files_dont_count_against_the_shrink(self):
        """With room reserved, the shrink succeeds although the engine's own
        files, still on macOS's volume, raised the minimum Asahi computes."""
        mac, doc, plan, outcome, channel, before = self.run_at(staging=8 * S.MIB, extra=16 * S.MIB)
        self.assertEqual(outcome, "waiting-for-recovery", channel.messages()[-2]["message"]["payload"])
        container = [k for k in doc["disk"]["containers"] if k["resize"]][0]
        to = plan["actions"][0]["to"]
        self.assertLess(to - container["resize"]["minimum"], 16 * S.MIB, "the minimum did rise above the plan's size")
        self.assertEqual(mac.ordered()[1].size, to)


class FreeSpaceRoom(unittest.TestCase):
    """A free-space plan doesn't shrink macOS, so Apple's files go in what
    the macOS container has free, less MACOS_MARGIN: the room plan.rs's
    derive_plan keeps there beside them."""

    def run_with_free(self, free):
        t = S.TempDir()
        self.addCleanup(t.cleanup)
        mac = S.j514s_free_space(t.sub("mac"))
        [macos] = [k for k in mac.containers.values() if mac.is_macos_container(k)]
        macos.free = free
        asahi, host = S.load_asahi(mac)
        _, doc, _ = S.inspect(mac, asahi, host, t.sub("w0"))
        release = S.make_release(t.sub("assets"))
        plan = S.derive_plan(doc, release, 150 * S.GIB, place="free-space")
        before = mac.state_digest()
        outcome, channel = S.run_engine(
            "execute", mac, asahi, host, t.sub("work"), plan=plan, release=release, restore=S.make_restore_zip(BASESYSTEM)
        )
        failures = [m["message"]["payload"] for m in channel.messages() if m["message"]["type"] == "failure"]
        return outcome, (failures[0] if failures else None), mac.state_digest() == before

    def test_apples_files_fit_beside_the_margin_or_nothing_changes(self):
        # What the members need, as the engine counts it, from a run with no
        # room at all.
        outcome, failure, unchanged = self.run_with_free(c.MACOS_MARGIN)
        self.assertEqual((outcome, failure["failure"]["code"], unchanged), ("failed", "apple-inputs-no-room", True))
        needed = int(re.search(r"need (\d+) bytes", failure["failure"]["message"]).group(1))
        outcome, failure, unchanged = self.run_with_free(c.MACOS_MARGIN + needed - 1)
        self.assertEqual((failure["stage"], failure["failure"]["code"], failure["failure"]["disk_changed"]), ("apple-inputs", "apple-inputs-no-room", "no"))
        self.assertIn("free some space in macOS", failure["failure"]["message"])
        self.assertTrue(unchanged)
        outcome, failure, _ = self.run_with_free(c.MACOS_MARGIN + needed)
        self.assertEqual(outcome, "waiting-for-recovery", failure)


if __name__ == "__main__":
    unittest.main()
