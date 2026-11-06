# SPDX-License-Identifier: GPL-3.0-or-later
"""Setup hints: macOS's settings mapped to Linux's, or left out."""

import unittest

import support  # noqa: F401
from sekka_engine import hints


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


if __name__ == "__main__":
    unittest.main()
