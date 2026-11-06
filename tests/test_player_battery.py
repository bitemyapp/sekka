"""Numerical and invalid-run checks; these do not replace real playback evidence."""

import importlib.util
from pathlib import Path
import unittest


spec = importlib.util.spec_from_file_location("player_battery", Path(__file__).parents[1] / "scripts/player_battery.py")
bench = importlib.util.module_from_spec(spec)
spec.loader.exec_module(bench)


class BatteryTests(unittest.TestCase):
    def test_mirrored_order_is_balanced_without_overwriting_runs(self):
        players = ["utsushi", "vlc", "mpv"]
        runs = bench.schedule(players, [2435], players + players[::-1])
        self.assertEqual([r["player"] for r in runs], ["utsushi", "vlc", "mpv", "mpv", "vlc", "utsushi"])
        self.assertEqual(len({(r["run"], r["player"]) for r in runs}), 6)
        for player in players:
            self.assertEqual(sum(r["run"] for r in runs if r["player"] == player) / 2, 3.5)
        with self.assertRaises(ValueError):
            bench.schedule(players, [600, 2400], players)

    def test_irregular_intervals_use_time_weighted_power(self):
        samples = [dict(elapsed_s=t, battery=dict(energy_wh=50 - 10 * t / 3600,
                                                full_wh=60, power_w=p))
                   for t, p in [(0, 8), (10, 12), (60, 8)]]
        result = bench.estimate(samples)
        self.assertAlmostEqual(result["mean_power_w"], 10)
        self.assertAlmostEqual(result["endpoint_power_w"], 10)
        self.assertAlmostEqual(result["slope_power_w"], 10)
        self.assertAlmostEqual(result["projected_full_minutes"], 360)

    def test_quantized_battery_still_has_correct_longer_slope(self):
        samples = [dict(elapsed_s=t, battery=dict(energy_wh=round(50 - 12 * t / 3600, 2),
                                                full_wh=58, power_w=12)) for t in range(0, 121, 2)]
        result = bench.estimate(samples)
        self.assertAlmostEqual(result["slope_power_w"], 12, delta=0.03)

    def test_rejects_frozen_energy_and_time(self):
        s = dict(elapsed_s=0, battery=dict(energy_wh=50, full_wh=60, power_w=10))
        with self.assertRaises(ValueError):
            bench.estimate([s, s])
        with self.assertRaises(ValueError):
            bench.estimate([s, dict(s, elapsed_s=60)])

    def test_external_power_brightness_and_reserve_abort(self):
        b = dict(status="Discharging", online={"AC": 0}, capacity=50)
        s = dict(battery=b, brightness={"panel": 149})
        bench.ensure_environment(s, s["brightness"], 15)
        for changed in [dict(b, status="Charging"), dict(b, online={"AC": 1}), dict(b, capacity=15)]:
            with self.assertRaises(RuntimeError):
                bench.ensure_environment(dict(s, battery=changed), s["brightness"], 15)
        with self.assertRaises(RuntimeError):
            bench.ensure_environment(s, {"panel": 150}, 15)


if __name__ == "__main__":
    unittest.main()
