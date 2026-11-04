"""Unit tests for the agent-evaluation harness's pure parts (no VM, no model).

    python3 -m unittest discover -s tests/agent-evals -p test_harness.py
"""

import json
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent / "harness"))

import agent  # noqa: E402
import scenarios  # noqa: E402
import vm  # noqa: E402


class TextToolCalls(unittest.TestCase):
    names = {"shell", "finish"}

    def test_tagged(self):
        text = 'Let me look.\n<tool_call>\n{"name": "shell", "arguments": {"command": "df -h"}}\n</tool_call>'
        self.assertEqual(agent.text_tool_calls(text, self.names), [{"name": "shell", "arguments": {"command": "df -h"}}])

    def test_fenced(self):
        text = '```json\n{"name": "finish", "arguments": {"root_cause": "x", "changes": "y", "fixed": true}}\n```'
        self.assertEqual(agent.text_tool_calls(text, self.names)[0]["name"], "finish")

    def test_bare_object(self):
        self.assertEqual(len(agent.text_tool_calls('{"name": "shell", "arguments": {"command": "ls"}}', self.names)), 1)

    def test_unknown_names_and_prose_are_ignored(self):
        self.assertEqual(agent.text_tool_calls('<tool_call>{"name": "rm", "arguments": {}}</tool_call>', self.names), [])
        self.assertEqual(agent.text_tool_calls("I think the disk is full.", self.names), [])
        self.assertEqual(agent.text_tool_calls("", self.names), [])


class Images(unittest.TestCase):
    def test_only_the_newest_are_kept(self):
        img = {"type": "image_url", "image_url": {"url": "data:"}}
        messages = [{"role": "user", "content": [{"type": "text", "text": "t"}, dict(img)]} for _ in range(5)]
        agent.drop_old_images(messages, keep=2)
        kept = [p for m in messages for p in m["content"] if p["type"] == "image_url"]
        self.assertEqual(len(kept), 2)
        self.assertEqual(messages[0]["content"][1]["type"], "text")
        self.assertEqual(messages[-1]["content"][1]["type"], "image_url")


class Points(unittest.TestCase):
    def test_conventions(self):
        self.assertEqual(vm.to_screen(500, 500, "norm1000"), (720.0, 450.0))
        self.assertEqual(vm.to_screen(0.25, 0.5, "norm1"), (360.0, 450.0))
        self.assertEqual(vm.to_screen(100, 200, "pixel"), (100.0, 200.0))


class Refused(unittest.TestCase):
    def refused(self, command):
        return vm.refusal(command) is not None

    def test_ending_the_machine_or_session(self):
        for c in ["reboot", "systemctl reboot", "shutdown -h now", "loginctl terminate-user alice",
                  "nixos-rebuild switch", "rm -rf /", "rm -rf /home/alice", "ls; reboot", "sudo poweroff",
                  "true && systemctl --no-pager reboot"]:
            self.assertTrue(self.refused(c), c)

    def test_ordinary_commands(self):
        for c in ["systemctl restart notes-web", "rm -f /srv/data/logs/sync-debug.log", "journalctl -b --no-pager",
                  "systemctl --user status pipewire", "rm -rf /tmp/verify",
                  "systemctl list-unit-files | grep -viE 'reboot|poweroff|halt'", "echo shutdown"]:
            self.assertFalse(self.refused(c), c)


class Diagnosis(unittest.TestCase):
    def test_every_entry_needs_a_match(self):
        s = scenarios.SCENARIOS["nix-daemon-masked"]
        self.assertTrue(scenarios.diagnosed(s, {"root_cause": "nix-daemon.socket was masked", "changes": ""}))
        self.assertFalse(scenarios.diagnosed(s, {"root_cause": "the daemon socket", "changes": ""}))

    def test_same_complaint_different_causes(self):
        full = scenarios.SCENARIOS["disk-full"]
        typo = scenarios.SCENARIOS["backup-env-typo"]
        self.assertEqual(full.prompt, typo.prompt)
        report = {"root_cause": "BACKUP_DEST in /etc/sekka-backup.env said /srv/data/backpus"}
        self.assertTrue(scenarios.diagnosed(typo, report))
        self.assertFalse(scenarios.diagnosed(full, report))


class Registry(unittest.TestCase):
    def test_scenarios_are_complete(self):
        for s in scenarios.SCENARIOS.values():
            self.assertIn(s.track, ("diagnostics", "computer-use"))
            self.assertIn(s.area, ("sekka", "aizome"))
            if s.track == "diagnostics":
                self.assertTrue(s.diagnosis, s.id)
                self.assertIsNotNone(s.oracle, s.id)
            self.assertTrue(s.prompt.strip(), s.id)

    def test_tool_schemas_are_json(self):
        for tools in (vm.ShellTools(None), vm.DesktopTools(None)):
            json.dumps(tools.schemas())
            names = [t["function"]["name"] for t in tools.schemas()]
            self.assertIn("finish", names)
            self.assertEqual(len(names), len(set(names)))


if __name__ == "__main__":
    unittest.main()
