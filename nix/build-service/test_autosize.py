"""The controller's free-space CAS bound, without a deployment."""
import importlib.machinery
import importlib.util
import os
from pathlib import Path
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


class DeferResize(unittest.TestCase):
    def test_waits_for_an_idle_service(self):
        self.assertTrue(controller.defer_resize(3, 80 * GiB, 100 * GiB))
        self.assertFalse(controller.defer_resize(0, 80 * GiB, 100 * GiB))

    def test_disk_pressure_cannot_wait(self):
        self.assertFalse(controller.defer_resize(3, 49 * GiB, 100 * GiB))


class FakeDocker:
    """Records docker calls; `fail` names a subcommand that raises."""
    def __init__(self, states, fail=None):
        self.states, self.fail, self.calls = states, fail, []

    def __call__(self, m, *args, capture=False, input=None):
        self.calls.append(args)
        if args[0] == self.fail:
            raise controller.subprocess.CalledProcessError(1, args)
        return ""

    def inspect(self, m, container):
        state = self.states.get(container)
        return state and {"State": {"Status": state, "Running": state in ("running", "paused"),
                                    "Paused": state == "paused"}}


class GatewayHold(unittest.TestCase):
    m = {"name": "shared-build-service-0123456789ab", "domains": {"trusted": {}, "ci": {}}}

    def run_with(self, fake, call):
        saved = controller.docker, controller.inspect_owned, controller.assert_owned
        controller.docker, controller.inspect_owned = fake, fake.inspect
        controller.assert_owned = lambda m, container: None
        handlers = {n: controller.signal.getsignal(n) for n in (controller.signal.SIGTERM, controller.signal.SIGHUP)}
        try:
            return call()
        finally:
            controller.docker, controller.inspect_owned, controller.assert_owned = saved
            for number, handler in handlers.items():
                self.assertIs(controller.signal.getsignal(number), handler)

    def gateway(self, domain):
        return controller.name(self.m, domain, "gateway")

    def test_a_failed_restart_resumes_every_gateway(self):
        fake = FakeDocker({self.gateway("trusted"): "running", self.gateway("ci"): "running"}, fail="start")
        with self.assertRaises(controller.subprocess.CalledProcessError):
            self.run_with(fake, lambda: controller.restart_backends(self.m, 1))
        unpaused = [args[1] for args in fake.calls if args[0] == "unpause"]
        self.assertEqual(sorted(unpaused), sorted([self.gateway("trusted"), self.gateway("ci")]))

    def test_a_gateway_left_paused_is_resumed(self):
        fake = FakeDocker({self.gateway("trusted"): "paused", self.gateway("ci"): "running"})
        self.run_with(fake, lambda: controller.restart_backends(self.m, 1))
        self.assertNotIn(("pause", self.gateway("trusted")), fake.calls)
        self.assertIn(("unpause", self.gateway("trusted")), fake.calls)
        fake = FakeDocker({self.gateway("trusted"): "paused"})
        self.run_with(fake, lambda: controller.run_container(self.m, "trusted", "gateway"))
        self.assertEqual(fake.calls, [("unpause", self.gateway("trusted"))])


class LogicalBytes(unittest.TestCase):
    def test_skips_blobs_evicted_during_the_scan(self):
        import tempfile
        with tempfile.TemporaryDirectory() as directory:
            content = os.path.join(directory, "trusted/data/cas/content/d2")
            os.makedirs(content)
            for index, size in enumerate((100, 200)):
                with open(os.path.join(content, f"blob-{index}"), "wb") as blob:
                    blob.write(b"x" * size)
            real = controller.os.scandir

            def evicting(path):
                entries = list(real(path))
                if str(path).endswith("d2"):
                    os.unlink(entries[0].path)  # NativeLink evicts after the listing.
                return iter(entries)
            controller.os.scandir = evicting
            try:
                total = controller.cas_logical_bytes({"directory": directory, "domains": {"trusted": {}}})
            finally:
                controller.os.scandir = real
            self.assertIn(total, (100, 200))


class Dispatch(unittest.TestCase):
    def test_main_does_not_shadow_module_functions(self):
        # A subparser named like a helper turned `resize` into a parser object.
        import ast
        tree = ast.parse(Path(loader.path).read_text())
        functions = {node.name for node in tree.body if isinstance(node, ast.FunctionDef)}
        main = next(node for node in tree.body if isinstance(node, ast.FunctionDef) and node.name == "main")
        assigned = {target.id for node in ast.walk(main) if isinstance(node, ast.Assign)
                    for target in node.targets if isinstance(target, ast.Name)}
        self.assertEqual(functions & assigned, set())


if __name__ == "__main__":
    unittest.main()
