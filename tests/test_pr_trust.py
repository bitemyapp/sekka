import copy
import importlib.util
import os
from pathlib import Path
import subprocess
import unittest
from urllib.error import HTTPError


ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("pr_trust", ROOT / "scripts/check-pr-trust.py")
gate = importlib.util.module_from_spec(spec)
spec.loader.exec_module(gate)
HEAD, BASE, MERGE = "a" * 40, "b" * 40, "c" * 40


class TrustGate(unittest.TestCase):
    def setUp(self):
        self.pr = {"state": "open", "draft": False, "user": {"login": "author"},
                   "head": {"sha": HEAD, "repo": {"full_name": gate.REPOSITORY}},
                   "base": {"sha": BASE, "ref": "main", "repo": {"full_name": gate.REPOSITORY}},
                   "mergeable": True, "merge_commit_sha": MERGE}
        self.roles = {"author": "write", "actor": "admin", "rerunner": "write"}
        self.current_base = BASE
        self.parents = [BASE, HEAD]
        self.calls = []

    def api(self, path):
        self.calls.append(path)
        if path.startswith("collaborators/"):
            return {"permission": self.roles[path.split("/")[1]]}
        if path == "pulls/233":
            return copy.deepcopy(self.pr)
        if path == "git/ref/heads/main":
            return {"object": {"sha": self.current_base}}
        if path == f"git/commits/{MERGE}":
            return {"parents": [{"sha": sha} for sha in self.parents]}
        self.fail(f"Unexpected API request: {path}")

    def select(self, **kwargs):
        return gate.select_merge(233, "actor", "rerunner", HEAD, get=self.api,
                                 sleep=lambda _: None, **kwargs)

    def test_current_writers_get_only_verified_merge(self):
        self.assertEqual(self.select(), {"head": HEAD, "base": BASE, "merge": MERGE})
        for user in self.roles:
            self.assertIn(f"collaborators/{user}/permission", self.calls)

    def test_read_triage_and_revoked_permissions_are_denied_for_each_identity(self):
        for user in self.roles:
            original = self.roles[user]
            for role in ["read", "none", "triage", "unknown"]:
                with self.subTest(user=user, role=role):
                    self.roles[user] = role
                    with self.assertRaises(gate.NotTrusted):
                        self.select()
            self.roles[user] = original

    def test_fork_deleted_repository_wrong_base_and_stale_head_are_denied(self):
        original = copy.deepcopy(self.pr)
        for side, field, value in [("head", "repo", {"full_name": "author/sekka"}),
                                   ("head", "repo", None), ("base", "ref", "nightly"),
                                   ("base", "repo", {"full_name": "other/sekka"}),
                                   ("head", "sha", "d" * 40)]:
            with self.subTest(side=side, field=field, value=value):
                self.pr = copy.deepcopy(original)
                self.pr[side][field] = value
                with self.assertRaises(gate.NotTrusted):
                    self.select()

    def test_closed_draft_conflicted_and_unready_prs_are_denied(self):
        original = copy.deepcopy(self.pr)
        for field, value in [("state", "closed"), ("draft", True),
                             ("mergeable", False), ("mergeable", None),
                             ("merge_commit_sha", None), ("merge_commit_sha", "main")]:
            with self.subTest(field=field):
                self.pr = {**original, field: value}
                with self.assertRaises(gate.NotTrusted):
                    self.select()

    def test_stale_or_unrelated_merge_parents_are_rejected(self):
        for parents in [[BASE, "d" * 40], ["d" * 40, HEAD], [HEAD, BASE], [BASE]]:
            self.parents = parents
            with self.assertRaises(gate.NotTrusted):
                self.select()

    def test_stale_pr_base_does_not_reject_a_merge_with_current_main(self):
        self.pr["base"]["sha"] = "d" * 40
        self.assertEqual(self.select(), {"head": HEAD, "base": BASE, "merge": MERGE})

    def test_merge_with_stale_main_is_rejected_even_if_pr_metadata_matches(self):
        self.current_base = "d" * 40
        with self.assertRaises(gate.NotTrusted):
            self.select()

    def test_rechecks_main_while_waiting_for_an_updated_merge(self):
        self.current_base = "d" * 40
        def delayed(path):
            if path == f"git/commits/{MERGE}" and self.calls.count(path) == 1:
                self.parents = [self.current_base, HEAD]
            return self.api(path)
        selected = gate.select_merge(233, "actor", "rerunner", HEAD, get=delayed,
                                     sleep=lambda _: None)
        self.assertEqual(selected, {"head": HEAD, "base": self.current_base, "merge": MERGE})
        self.assertEqual(self.calls.count("git/ref/heads/main"), 2)

    def test_current_main_lookup_errors_fail_closed(self):
        def failed(path):
            if path == "git/ref/heads/main":
                error = HTTPError("https://api.github.com", 403, "unavailable", {}, None)
                self.addCleanup(error.close)
                raise error
            return self.api(path)
        with self.assertRaises(HTTPError):
            gate.select_merge(233, "actor", "rerunner", HEAD, get=failed)

    def test_waits_for_github_to_compute_merge(self):
        def delayed(path):
            result = self.api(path)
            if path == "pulls/233" and self.calls.count(path) == 1:
                result["mergeable"] = None
            return result
        selected = gate.select_merge(233, "actor", "rerunner", HEAD, get=delayed,
                                     sleep=lambda _: None)
        self.assertEqual(selected["merge"], MERGE)
        self.assertEqual(self.calls.count("pulls/233"), 2)

    def test_api_errors_fail_closed(self):
        for code in [403, 404, 500]:
            def failed(_):
                error = HTTPError("https://api.github.com", code, "unavailable", {}, None)
                self.addCleanup(error.close)
                raise error
            with self.subTest(code=code), self.assertRaises((HTTPError, gate.NotTrusted)):
                gate.select_merge(233, "actor", "rerunner", get=failed)

    def test_invalid_number_never_calls_api(self):
        for number in ["0", "233/commits", "$(id)", "-1", ""]:
            with self.assertRaises(gate.NotTrusted):
                gate.select_merge(number, "actor", "rerunner", get=self.api)
        self.assertEqual(self.calls, [])


class RunnerAdmission(unittest.TestCase):
    def admitted(self, event, ref="refs/heads/main", repository=gate.REPOSITORY,
                 workflow="bitemyapp/sekka/.github/workflows/pr.yml@refs/heads/main"):
        env = {**os.environ, "GITHUB_REPOSITORY": repository, "GITHUB_REF": ref,
               "GITHUB_EVENT_NAME": event, "GITHUB_WORKFLOW_REF": workflow}
        return subprocess.run(["bash", "-c", 'source "$1"; sekka_runner_admitted',
                               "admission-test", str(ROOT / "scripts/runner-admission.sh")],
                              env=env, check=False).returncode == 0

    def test_existing_main_workflows_remain_admitted(self):
        for event in ["push", "schedule", "workflow_dispatch"]:
            self.assertTrue(self.admitted(event))
            self.assertFalse(self.admitted(event, ref="refs/heads/topic"))

    def test_only_main_trust_workflow_can_admit_pr_jobs(self):
        self.assertTrue(self.admitted("pull_request_target"))
        for event in ["pull_request", "workflow_run", "issue_comment", ""]:
            self.assertFalse(self.admitted(event))
        for workflow in ["", "bitemyapp/sekka/.github/workflows/evil.yml@refs/heads/main",
                         "bitemyapp/sekka/.github/workflows/pr.yml@refs/heads/topic"]:
            self.assertFalse(self.admitted("pull_request_target", workflow=workflow))
        self.assertFalse(self.admitted("pull_request_target", ref="refs/pull/233/merge"))
        self.assertFalse(self.admitted("pull_request_target", repository="other/sekka"))

    def test_every_main_workflow_is_admitted(self):
        # Both runner slots share this policy: it has no slot input.
        for event in ["push", "schedule", "workflow_dispatch"]:
            for workflow in ["pr", "cache", "nightly", "sites", "release", "yuki"]:
                with self.subTest(event=event, workflow=workflow):
                    self.assertTrue(self.admitted(event, workflow=
                        f"bitemyapp/sekka/.github/workflows/{workflow}.yml@refs/heads/main"))
            self.assertFalse(self.admitted(event, repository="other/sekka"))


if __name__ == "__main__":
    unittest.main()
