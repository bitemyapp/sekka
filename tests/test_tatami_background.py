#!/usr/bin/env python3
"""Exercise the real Tatami CLI with isolated state and a captured renderer."""

from concurrent.futures import ThreadPoolExecutor
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from PIL import Image

BINARY = sys.argv.pop(1)


class BackgroundTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.bin = self.root / "bin"
        self.bin.mkdir()
        self.env = dict(os.environ, XDG_STATE_HOME=str(self.root / "state"),
                        PATH=str(self.bin))
        self.restarts = self.root / "restarts"
        self.script("systemctl", f"from pathlib import Path; p = Path({str(self.restarts)!r}); p.write_bytes(p.read_bytes() + b'restart\\n' if p.exists() else b'restart\\n')")
        self.script("renderer", "import json, sys; print(json.dumps(sys.argv[1:]))")
        self.file = self.root / "state/tatami/background.json"
        self.link = self.file.with_name("background")
        self.image = self.root / "image with spaces;$(false).jpg"
        # Content, not the extension, decides whether the real decoder accepts it.
        Image.new("RGB", (32, 24), (40, 80, 120)).save(self.image, format="PNG")
        self.call("set", str(self.image))

    def script(self, name, body):
        script = self.bin / name
        script.write_text(f"#!{sys.executable}\n{body}\n")
        script.chmod(0o755)

    def call(self, *args, success=True):
        result = subprocess.run([BINARY, "background", *args], env=self.env,
                                text=True, capture_output=True)
        self.assertEqual(result.returncode == 0, success, result.stderr)
        return result

    def rendered(self):
        args = json.loads(self.call("run", str(self.bin / "renderer")).stdout)
        outputs = {}
        for index in range(0, len(args), 2):
            key, value = args[index:index + 2]
            if key == "-o":
                current = outputs.setdefault(value, {})
            else:
                current[key] = value
        return outputs

    def test_modes_persist_across_processes_and_wallpaper_changes(self):
        for mode in ["fill", "fit", "stretch", "center", "tile", "solid_color"]:
            self.call("mode", mode)
            self.call("color", "AABBCC")
            self.call("set", str(self.image))
            actual = self.rendered()["*"]
            self.assertEqual(actual["-m"], mode)
            self.assertEqual(actual["-c"], "#aabbcc")
            self.assertEqual("-i" in actual, mode != "solid_color")
            self.assertEqual(self.link.resolve(), self.image)

    def test_output_inheritance_missing_file_and_reset(self):
        other = self.root / "portrait.jpg"
        other.write_bytes(self.image.read_bytes())
        self.call("mode", "fit")
        self.call("color", "112233")
        self.call("mode", "center", "--output", "DP-1")
        self.call("set", str(other), "--output", "DP-1")
        self.call("color", "445566", "--output", "eDP-1")
        self.call("mode", "tile")
        outputs = self.rendered()
        self.assertEqual(outputs["DP-1"], {"-m": "center", "-c": "#112233", "-i": str(other)})
        self.assertEqual(outputs["eDP-1"]["-m"], "tile")
        self.assertEqual(outputs["eDP-1"]["-c"], "#445566")
        other.unlink()
        self.assertEqual(self.rendered()["DP-1"]["-i"], str(self.link))
        self.assertEqual(json.loads(self.file.read_text())["outputs"]["DP-1"]["image"], str(other))
        self.call("reset", "--output", "DP-1")
        self.assertNotIn("DP-1", self.rendered())
        self.assertIn("eDP-1", self.rendered())
        self.call("reset")
        self.assertEqual(set(self.rendered()), {"*"})
        self.assertEqual(self.rendered()["*"]["-m"], "fill")
        self.assertEqual(self.link.resolve(), self.image)

    def test_invalid_edits_do_not_overwrite_valid_settings(self):
        self.call("mode", "fit")
        before = self.file.read_bytes()
        for args in [("mode", "zoom"), ("color", "red"), ("set", "relative.jpg"),
                     ("mode", "tile", "--output", "*"), ("mode", "fit", "unexpected")]:
            self.call(*args, success=False)
            self.assertEqual(self.file.read_bytes(), before)
        self.file.write_text("{invalid json")
        self.call("color", "112233", success=False)
        self.assertEqual(self.file.read_text(), "{invalid json")
        self.assertEqual(self.rendered()["*"]["-m"], "fill")
        self.assertEqual(self.file.read_text(), "{invalid json")
        self.call("reset")
        self.assertEqual(json.loads(self.file.read_text())["mode"], "fill")

    def test_concurrent_display_edits_are_not_lost(self):
        with ThreadPoolExecutor(max_workers=4) as pool:
            list(pool.map(lambda i: self.call("mode", "fit", "--output", f"DP-{i}"), range(1, 9)))
        self.assertEqual(len(self.rendered()), 9)  # Eight overrides and wildcard defaults.

    def test_real_decoding_accepts_renderer_formats(self):
        for format in ["PNG", "JPEG", "WEBP", "BMP", "GIF"]:
            image = self.root / format.lower()
            Image.new("RGB", (32, 24), (160, 80, 40)).save(image, format=format)
            for output in [(), ("--output", "DP-1")]:
                with self.subTest(format=format, output=output):
                    self.call("set", str(image), *output)
                    actual = self.rendered()["DP-1" if output else "*"]["-i"]
                    self.assertEqual(Path(actual).resolve(), image)
        svg = self.root / "vector.svg"
        svg.write_text('<svg xmlns="http://www.w3.org/2000/svg" width="32" height="24"><rect width="32" height="24" fill="red"/></svg>')
        self.call("set", str(svg))
        self.assertEqual(self.link.resolve(), svg)

    def test_bad_images_preserve_choice_settings_and_renderer(self):
        self.call("mode", "fit")
        self.call("set", str(self.image), "--output", "DP-1")
        bad = self.root / "not an image.png"
        bad.write_text("plain text, despite the extension")
        truncated = self.root / "truncated.png"
        truncated.write_bytes(self.image.read_bytes()[:40])
        unreadable = self.root / "unreadable.png"
        unreadable.write_bytes(self.image.read_bytes())
        unreadable.chmod(0)
        for image in [bad, truncated, unreadable, self.root / "missing.png", self.root]:
            for output in [(), ("--output", "DP-1")]:
                with self.subTest(image=image.name, output=output):
                    self.call("set", str(self.image))
                    self.call("set", str(self.image), "--output", "DP-1")
                    before = (self.file.read_bytes(), os.readlink(self.link), self.restarts.read_bytes())
                    result = self.call("set", str(image), *output, success=False)
                    self.assertIn("Wallpaper", result.stderr)
                    self.assertEqual((self.file.read_bytes(), os.readlink(self.link), self.restarts.read_bytes()), before)
        # Rejection must also leave a first-time user's state directory absent.
        self.env["XDG_STATE_HOME"] = str(self.root / "fresh-state")
        self.call("set", str(truncated), success=False)
        self.assertFalse((self.root / "fresh-state").exists())

    def test_settings_menu_persists_selected_display_mode(self):
        queue = self.root / "selections.json"
        queue.write_text(json.dumps(["1", "0", "1", ""]))  # DP-1, scaling, fit, close.
        self.script("hyprctl", 'print(\'[ {"name": "DP-1"} ]\')')
        self.script("walker", f"""import json
from pathlib import Path
p = Path({str(queue)!r})
selections = json.loads(p.read_text())
print(selections.pop(0))
p.write_text(json.dumps(selections))""")
        self.call("settings")
        self.assertEqual(self.rendered()["DP-1"]["-m"], "fit")
        self.assertEqual(self.rendered()["*"]["-m"], "fill")


if __name__ == "__main__":
    unittest.main()
