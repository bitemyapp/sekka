import importlib.util
from pathlib import Path
import sys
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location(
    "agent_workloads", Path(__file__).resolve().parents[1] / "nix/scheduling/agent-workloads.py"
)
policy = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = policy
spec.loader.exec_module(policy)


class WorkloadPolicyTest(unittest.TestCase):
    def groups(self, *processes):
        return {
            (name, kind): {p.pid for p in members}
            for (name, kind), members in policy.scopes({p.pid: p for p in processes}).items()
        }

    def test_interactive_roots_background_descendants_and_nested_harness(self):
        p = policy.Process
        groups = self.groups(
            p(1, 0, 1, False), p(10, 1, 10, True),
            p(11, 10, 11, False), p(12, 11, 12, False),
            p(13, 11, 13, True), p(14, 13, 14, False),
            p(20, 1, 20, False), p(21, 20, 21, False),
        )
        self.assertEqual(groups, {
            ("app-sekka-agent-10-10.scope", "agent"): {10},
            ("app-sekka-work-11-11.scope", "work"): {11, 12},
            ("app-sekka-agent-13-13.scope", "agent"): {13},
            ("app-sekka-work-14-14.scope", "work"): {14},
        })

    def test_reused_pid_has_a_different_unit_identity(self):
        p = policy.Process
        before = self.groups(p(10, 1, 1, True), p(11, 10, 2, False))
        after = self.groups(p(10, 1, 1, True), p(11, 10, 3, False))
        self.assertNotEqual(before.keys(), after.keys())

    def test_do_not_move_the_harness_out_of_its_launcher(self):
        with patch.object(policy, "group", return_value=policy.MANAGER + "app.slice/terminal.scope"), \
             patch.object(policy.subprocess, "run") as run:
            policy.attach("busctl", "app-sekka-agent-10-1.scope", "agent", [policy.Process(10, 1, 1, True)])
            run.assert_not_called()

    def test_leave_foreign_managers_and_delegated_subgroups_alone(self):
        for group in ("/system.slice/session-1.scope", policy.MANAGER + "app.slice/container.service/delegated"):
            with patch.object(policy, "group", return_value=group), \
                 patch.object(policy.subprocess, "run") as run:
                policy.attach("busctl", "app-sekka-work-10-1.scope", "work", [policy.Process(10, 1, 1, False)])
                run.assert_not_called()

    def test_nixos_wrapper_names(self):
        self.assertEqual(policy.basename("/nix/store/package/bin/.claude-wrapped"), "claude")
        self.assertEqual(policy.basename("codex"), "codex")


if __name__ == "__main__":
    unittest.main()
