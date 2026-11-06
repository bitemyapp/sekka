"""The first start's identity record and the boot-entry filling, against
temporary files. Run: python3 -m unittest test_install_identity."""

import json
import tempfile
import unittest
from pathlib import Path

import install_identity as ii

ROOT = "63d89be1-72f7-4aff-9679-09cafa7e0c2d"
ESP = "910de5ca-7390-4716-9a54-8028916e4d78"
OTHER = "0b8a6d1e-2f61-4c55-8f7e-3a3c2c1d9e01"


def handoff(**changes):
    value = {
        "schema": "sekka.mac.handoff/1",
        "installation": "01010101-0101-0101-0101-010101010101",
        "release": "sekka-j514s-4882dc33",
        "source_revision": "4882dc33652612e7475f467f6373d8b607e5407c",
        "root": ROOT,
        "esp": ESP,
        "created": "2026-11-06T21:00:00Z",
        "hints": {"computer_name": "Chris's MacBook Pro", "full_name": None, "keyboard": "us",
                  "locale": "en_US.UTF-8", "timezone": "America/Chicago"},
    }
    value.update(changes)
    return ii.canonical(value)


CMDLINE = f"init=/nix/store/x/init sekka.root=PARTUUID={ROOT} sekka.esp=PARTUUID={ESP} quiet\n"
MOUNTED = {"root": ROOT, "esp": ESP}


class Handoff(unittest.TestCase):
    def test_contract_sample_parses(self):
        # The same document as handoff.rs's sample(), in its canonical bytes.
        self.assertEqual(ii.parse_handoff(handoff())["root"], ROOT)

    def test_refusals(self):
        bad = {
            "not canonical": handoff().replace(b'","', b'", "', 1),
            "another schema": handoff(schema="sekka.mac.handoff/2"),
            "unknown field": ii.canonical({**json.loads(handoff()), "password": "x"}),
            "same partition": handoff(esp=ROOT),
            "uppercase uuid": handoff(root=ROOT.upper()),
            "short revision": handoff(source_revision="4882dc33"),
            "too large": handoff() + b" " * ii.HANDOFF_LIMIT,
            "not json": b"{",
        }
        for why, data in bad.items():
            with self.subTest(why), self.assertRaises(ii.Refused):
                ii.parse_handoff(data)

    def test_cmdline(self):
        self.assertEqual(ii.from_cmdline(CMDLINE), MOUNTED)
        for text in [
            "quiet",
            CMDLINE.replace(ROOT, "@SEKKA_ROOT_PARTUUID@"),
            CMDLINE + f" sekka.root=PARTUUID={ROOT}",
            CMDLINE.replace(f"PARTUUID={ESP}", ESP),
        ]:
            with self.subTest(text), self.assertRaises(ii.Refused):
                ii.from_cmdline(text)


class Record(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.nixos = Path(self.tmp.name)

    def tearDown(self):
        self.tmp.cleanup()

    def test_writes_once_then_checks(self):
        self.assertIn("wrote", ii.record(handoff(), CMDLINE, MOUNTED, self.nixos))
        text = (self.nixos / "identity.nix").read_text()
        self.assertEqual(ii.read_identity_nix(text), MOUNTED)
        self.assertIn('root = "' + ROOT + '";', text)
        self.assertIn("already", ii.record(handoff(), CMDLINE, MOUNTED, self.nixos))
        self.assertEqual((self.nixos / "identity.nix").read_text(), text)

    def test_refuses_another_installations_file(self):
        (self.nixos / "identity.nix").write_text(ii.identity_nix(OTHER, ESP, "x"))
        with self.assertRaises(ii.Refused):
            ii.record(handoff(), CMDLINE, MOUNTED, self.nixos)
        self.assertIn(OTHER, (self.nixos / "identity.nix").read_text())

    def test_refuses_disagreement(self):
        cases = {
            "handoff names another root": (handoff(root=OTHER), CMDLINE, MOUNTED),
            "another root is mounted": (handoff(), CMDLINE, {"root": OTHER, "esp": ESP}),
            "the ESP isn't a GPT partition": (handoff(), CMDLINE, {"root": ROOT, "esp": None}),
        }
        for why, (h, c, m) in cases.items():
            with self.subTest(why), self.assertRaises(ii.Refused):
                ii.record(h, c, m, self.nixos)
            self.assertFalse((self.nixos / "identity.nix").exists())


class Entries(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.esp = Path(self.tmp.name)
        (self.esp / "loader" / "entries").mkdir(parents=True)

    def tearDown(self):
        self.tmp.cleanup()

    def entry(self, name, options):
        path = self.esp / "loader" / "entries" / name
        path.write_text(f"title NixOS\nlinux /EFI/nixos/k.efi\noptions init=/x/init {options}\n")
        return path

    def test_fills_placeholders_only(self):
        first = self.entry("nixos-a.conf", "sekka.root=PARTUUID=@SEKKA_ROOT_PARTUUID@ sekka.esp=PARTUUID=@SEKKA_ESP_PARTUUID@")
        later = self.entry("nixos-b.conf", f"sekka.root=PARTUUID={ROOT} sekka.esp=PARTUUID={ESP}")
        before = later.read_text()
        self.assertEqual(ii.fill_entries(self.esp, MOUNTED), ["nixos-a.conf"])
        self.assertIn(f"sekka.root=PARTUUID={ROOT} sekka.esp=PARTUUID={ESP}", first.read_text())
        self.assertEqual(later.read_text(), before)
        self.assertEqual(ii.unfilled(self.esp), [])

    def test_identity_for_entries(self):
        self.assertEqual(ii.entry_identity(CMDLINE, None), MOUNTED)
        self.assertEqual(ii.entry_identity(CMDLINE, MOUNTED), MOUNTED)
        # A rescue system without the arguments uses identity.nix's.
        self.assertEqual(ii.entry_identity("quiet", MOUNTED), MOUNTED)
        with self.assertRaises(ii.Refused):
            ii.entry_identity("quiet", None)
        with self.assertRaises(ii.Refused):
            ii.entry_identity(CMDLINE, {"root": OTHER, "esp": ESP})

    def test_twice_is_refused(self):
        self.entry("nixos-a.conf", "sekka.root=PARTUUID=@SEKKA_ROOT_PARTUUID@ x=@SEKKA_ROOT_PARTUUID@")
        with self.assertRaises(ii.Refused):
            ii.fill_entries(self.esp, MOUNTED)


if __name__ == "__main__":
    unittest.main()
