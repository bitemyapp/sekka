"""The controller's free-space CAS bound, without a deployment."""
import importlib.machinery
import importlib.util
import os
import unittest

loader = importlib.machinery.SourceFileLoader("controller", os.environ.get("SEKKA_BUILD_SERVICE", "build-service"))
spec = importlib.util.spec_from_loader("controller", loader)
controller = importlib.util.module_from_spec(spec)
loader.exec_module(controller)
GiB = 1024**3


class AutosizeTarget(unittest.TestCase):
    def target(self, current, configured, free, floor=100 * GiB):
        return controller.autosize_target(current * GiB, configured * GiB, free * GiB, floor, 4 * GiB, 256 * GiB)

    def test_grows_into_free_space_above_the_floor(self):
        self.assertEqual(self.target(10, 64, 200), 110 * GiB)

    def test_shrinks_below_current_under_pressure(self):
        self.assertEqual(self.target(60, 64, 40), 4 * GiB)  # 60 + 40 - 100 is below the minimum
        self.assertEqual(self.target(60, 64, 70), 30 * GiB)

    def test_small_changes_keep_the_bound(self):
        self.assertIsNone(self.target(10, 64, 152))  # 62 GiB: within 5% of 64
        self.assertIsNone(self.target(10, 64, 160))  # 70 GiB: grows only past 25%

    def test_respects_the_maximum(self):
        self.assertEqual(self.target(10, 64, 2000), 256 * GiB)


if __name__ == "__main__":
    unittest.main()
