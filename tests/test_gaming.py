"""Exercise argv boundaries and host-compositor policy without running a game."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

SCRIPT = Path(os.environ.get("SEKKA_GAME_SCRIPT", Path(__file__).resolve().parents[1] / "installer/genkan/rust/system/gaming/sekka-game.sh"))


class GameLauncher(unittest.TestCase):
    def run_game(self, *args, desktop="Aizome", gamescope=False):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            # Each wrapper records its arguments, then passes them through.
            for name in ("gamescope", "gamemoderun", "mangohud", "game"):
                script = root / name
                script.write_text(f"#!{sys.executable}\n" +
                    "import json, os, sys\n" +
                    "name=os.path.basename(sys.argv[0])\n" +
                    "with open(os.environ['TRACE'], 'a') as f: f.write(json.dumps([name, *sys.argv[1:]])+'\\n')\n" +
                    "args=sys.argv[1:]\n" +
                    "if name=='gamescope': args=args[args.index('--')+1:]\n" +
                    "if name!='game': os.execvp(args[0], args)\n")
                script.chmod(0o755)
            env = dict(os.environ, PATH=str(root) + ":" + os.environ["PATH"], TRACE=str(root / "trace"), XDG_CURRENT_DESKTOP=desktop)
            env.pop("GAMESCOPE_WAYLAND_DISPLAY", None)
            if gamescope:
                env["GAMESCOPE_WAYLAND_DISPLAY"] = "gamescope-0"
            result = subprocess.run(["bash", str(SCRIPT), *args], env=env, text=True, capture_output=True)
            trace = [json.loads(line) for line in (root / "trace").read_text().splitlines()] if (root / "trace").exists() else []
            return result, trace

    def test_default_preserves_every_argument(self):
        args = ["game", "space in name", "$(touch should-not-exist)", "", "--flag"]
        result, trace = self.run_game("--", *args)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(trace, [["gamemoderun", *args], args])

    def test_hud_and_nested_wrapper_order(self):
        result, trace = self.run_game("--gamescope", "--hud", "--tearing", "--", "game", desktop="Hyprland")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(trace[0], ["gamescope", "--fullscreen", "--immediate-flips", "--", "gamemoderun", "mangohud", "game"])
        self.assertEqual([t[0] for t in trace], ["gamescope", "gamemoderun", "mangohud", "game"])

    def test_mode_sets_game_resolution_and_refresh(self):
        result, trace = self.run_game("--mode", "2560x1440@144", "--", "game", desktop="Hyprland")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(trace[0], ["gamescope", "--fullscreen", "-W", "2560", "-H", "1440",
                                    "-w", "2560", "-h", "1440", "-r", "144", "--", "gamemoderun", "game"])
        self.assertNotIn("1280x720", result.stderr)
        result, trace = self.run_game("--gamescope", "--mode=1920x1080", "--", "game")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(trace[0][:10], ["gamescope", "--fullscreen", "-W", "1920", "-H", "1080",
                                         "-w", "1920", "-h", "1080"])
        self.assertEqual(trace[0][10], "--")
        for args in [["--mode"], ["--mode", "1920"], ["--mode", "0x1080", "--", "game"],
                     ["--mode=1920x1080@", "--", "game"], ["--mode", "big", "--", "game"]]:
            result, trace = self.run_game(*args)
            self.assertEqual(result.returncode, 2, args)
            self.assertEqual(trace, [])

    def test_nested_without_mode_names_the_default(self):
        result, trace = self.run_game("--gamescope", "--", "game")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(trace[0], ["gamescope", "--fullscreen", "--", "gamemoderun", "game"])
        self.assertIn("--mode", result.stderr)

    def test_does_not_nest_in_existing_gamescope(self):
        result, trace = self.run_game("--gamescope", "--hdr", "--", "game", gamescope=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual([t[0] for t in trace], ["gamemoderun", "game"])

    def test_hdr_refuses_sdr_aizome_and_accepts_hdr_host(self):
        result, trace = self.run_game("--hdr", "--", "game")
        self.assertEqual(result.returncode, 2)
        self.assertIn("SDR", result.stderr)
        self.assertEqual(trace, [])
        result, trace = self.run_game("--hdr", "--", "game", desktop="Hyprland")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("--hdr-enabled", trace[0])

    def test_opt_out_and_errors(self):
        result, trace = self.run_game("--no-gamemode", "--", "game")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(trace, [["game"]])
        for args in [[], ["--"], ["--typo"], ["game"]]:
            result, trace = self.run_game(*args)
            self.assertEqual(result.returncode, 2)
            self.assertEqual(trace, [])

    def test_tearing_requires_a_capable_host(self):
        result, trace = self.run_game("--tearing", "--", "game")
        self.assertEqual(result.returncode, 2)
        self.assertIn("allowTearing", result.stderr)
        self.assertEqual(trace, [])
        result, trace = self.run_game("--tearing", "--", "game", desktop="gamescope")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("running compositor", result.stderr)
        self.assertEqual([t[0] for t in trace], ["gamemoderun", "game"])


if __name__ == "__main__":
    unittest.main()
