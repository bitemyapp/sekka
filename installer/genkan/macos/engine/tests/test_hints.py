# SPDX-License-Identifier: GPL-3.0-or-later
"""Setup hints: macOS's settings mapped to Linux's, or left out."""

import unittest

import support  # noqa: F401
from sekka_engine import hints
from sekka_engine import host


class Hints(unittest.TestCase):
    def test_locales(self):
        self.assertEqual(hints.glibc_locale("en_US"), "en_US.UTF-8")
        self.assertEqual(hints.glibc_locale("de_DE@rg=chzzzz"), "de_DE.UTF-8")
        self.assertEqual(hints.glibc_locale("zh-Hans_CN"), "zh_CN.UTF-8")
        for unmapped in ("en", "English", "en_001", None, 7):
            self.assertIsNone(hints.glibc_locale(unmapped), unmapped)

    def test_keyboards(self):
        self.assertEqual(hints.xkb_layout("com.apple.keylayout.US"), "us")
        self.assertEqual(hints.xkb_layout("com.apple.keylayout.German"), "de")
        self.assertEqual(hints.xkb_layout("com.apple.keylayout.British-PC"), "gb")
        # A layout that needs an XKB variant isn't guessed.
        self.assertIsNone(hints.xkb_layout("com.apple.keylayout.Dvorak"))
        self.assertIsNone(hints.xkb_layout("com.apple.inputmethod.Kotoeri"))

    def test_time_zones(self):
        self.assertEqual(hints.iana_zone("/var/db/timezone/zoneinfo/America/Chicago"), "America/Chicago")
        self.assertEqual(hints.iana_zone("/usr/share/zoneinfo/Etc/GMT+5"), "Etc/GMT+5")
        self.assertEqual(hints.iana_zone("/var/db/timezone/zoneinfo/UTC"), "UTC")
        for bad in ("/etc/localtime", "/x/zoneinfo/../../etc/passwd", None):
            self.assertIsNone(hints.iana_zone(bad), bad)


class SmallFileReads(unittest.TestCase):
    """host.read_small_file, which reads files a less privileged account
    controls: it never follows a final link, never waits, never reads a
    device or folder, and stops at its limit."""

    def setUp(self):
        import os

        import support as S

        self.t = S.TempDir()
        self.dir = self.t.sub("prefs")
        self.me = {os.getuid()}

    def tearDown(self):
        self.t.cleanup()

    def path(self, name):
        import os

        return os.path.join(self.dir, name)

    def test_a_small_regular_file_of_the_owner_is_read(self):
        with open(self.path("ok.plist"), "wb") as f:
            f.write(b"<plist/>")
        self.assertEqual(host.read_small_file(self.path("ok.plist"), self.me), b"<plist/>")

    def test_everything_else_is_refused_at_once(self):
        import os

        os.mkfifo(self.path("fifo"))
        os.symlink("/dev/zero", self.path("zero"))
        os.symlink(self.path("fifo"), self.path("link"))
        os.mkdir(self.path("folder"))
        with open(self.path("big"), "wb") as f:
            f.write(b"x" * (host.SMALL_FILE_LIMIT + 1))
        with open(self.path("mine"), "wb") as f:
            f.write(b"x")
        cases = [("fifo", self.me), ("zero", self.me), ("link", self.me), ("folder", self.me), ("big", self.me), ("mine", {os.getuid() + 1})]
        for name, owners in cases:
            with self.subTest(name):
                with self.assertRaises(OSError):
                    host.read_small_file(self.path(name), owners)
        with self.assertRaises(OSError):
            host.read_small_file("/dev/zero", {0, os.getuid()})


if __name__ == "__main__":
    unittest.main()
