"""Behavioral tests; no network, Nix builds or systemd."""

import copy
import hashlib
import io
import json
import os
from pathlib import Path
import re
import stat
import tempfile
import unittest
from unittest import mock

import deploy

OLD = "/nix/store/" + "0" * 32 + "-sekka-discord-0.1.0"
NEW = "/nix/store/" + "1" * 32 + "-sekka-discord-0.1.0"
NEWER = "/nix/store/" + "2" * 32 + "-sekka-discord-0.1.0"
REV = "a" * 40
LATER = "b" * 40


def workflow_run(**fields):
    run = {"id": 7, "run_attempt": 1, "head_sha": REV, "head_branch": "main", "event": "workflow_dispatch",
           "status": "completed", "conclusion": "success", "path": ".github/workflows/yuki.yml",
           "repository": {"full_name": "bitemyapp/sekka"}, "head_repository": {"full_name": "bitemyapp/sekka"}}
    run.update(fields)
    return run


def job(**fields):
    return {"name": "build", "status": "completed", "conclusion": "success", "head_sha": REV, **fields}


class FakeGitHub(deploy.GitHub):
    """main's head is the latest run's commit, unless a test says main has moved on."""

    def __init__(self, runs, jobs=None):
        super().__init__("https://api.github.com", "bitemyapp/sekka")
        self.runs = runs
        self.jobs = [job()] if jobs is None else jobs
        self.moved = None
        self.ancestors = set()
        self.requests = []
        self.unchanged = 0

    def get(self, path, etag=None):
        self.requests.append(path)
        if "/jobs" in path:
            return {"total_count": len(self.jobs), "jobs": self.jobs}, None
        head = self.moved or (self.runs[0]["head_sha"] if self.runs else REV)
        if "/git/ref/heads/" in path:
            return {"ref": "refs/heads/main", "object": {"type": "commit", "sha": head}}, None
        if "/compare/" in path:
            base, sha = path.rsplit("/", 1)[1].split("...")
            behind = base == head and sha in self.ancestors
            return {"status": "behind" if behind else "diverged", "ahead_by": 0 if behind else 1, "behind_by": 3,
                    "merge_base_commit": {"sha": sha if behind else "f" * 40}}, None
        tag = '"' + hashlib.sha256(json.dumps(self.runs).encode()).hexdigest() + '"'
        if etag == tag:
            self.unchanged += 1
            return None, etag
        return {"total_count": len(self.runs), "workflow_runs": self.runs}, tag


class FakeSystem:
    """Simulated units: a broken package crash-loops the bot or fails monitor ticks."""

    def __init__(self):
        self.fallback = OLD
        self.profile = None
        self.result = NEW
        self.broken = set()
        self.broken_monitor = set()
        self.running = OLD
        self.invocation = 1
        self.ticks = 1
        self.time = 0
        self.built = []
        self.switches = []
        self.discarded = 0

    def clock(self):
        return self.time

    def sleep(self, seconds):
        self.time += seconds

    def build(self, rev):
        self.built.append(rev)
        if isinstance(self.result, Exception):
            raise self.result
        return self.result

    def deployed(self):
        return self.profile

    def current(self):
        return self.profile or self.fallback

    def switch(self, path):
        self.switches.append(path)
        self.profile = path

    def restore(self, previous):
        if previous is None:
            self.profile = None
        else:
            self.switch(previous)

    def discard_candidate(self):
        self.discarded += 1

    def request_restart(self):
        self.running = self.current()
        self.invocation += 1

    def healthy(self):
        return self.running not in self.broken

    def unit(self, name):
        if name == deploy.MONITOR_TIMER:
            return {"ActiveState": "active"}
        if name == deploy.MONITOR:
            self.ticks += 1
            ok = self.current() not in self.broken_monitor
            return {"InvocationID": str(self.ticks), "ActiveState": "inactive",
                    "Result": "success" if ok else "exit-code", "ExecMainStatus": "0" if ok else "1"}
        if self.running in self.broken:
            self.invocation += 1
            return {"InvocationID": str(self.invocation), "ActiveState": "activating"}
        return {"InvocationID": str(self.invocation), "ActiveState": "active"}


def listening_state():
    """A deployer that has already taken its first look and found no run."""
    return {**deploy.new_state(), "floor": 0}


class DeployTests(unittest.TestCase):
    def setUp(self):
        self.state = listening_state()
        self.saves = []
        self.system = FakeSystem()
        self.output = io.StringIO()
        patcher = mock.patch("sys.stdout", new=self.output)
        patcher.start()
        self.addCleanup(patcher.stop)

    def tick(self, github, now=1000):
        return deploy.tick(self.state, github, self.system, "yuki.yml", "main", now,
                           lambda: self.saves.append(copy.deepcopy(self.state)))

    def test_successful_run_is_built_switched_restarted_and_recorded(self):
        self.assertEqual(self.tick(FakeGitHub([workflow_run()])), "deployed")
        self.assertEqual(self.system.built, [REV])
        self.assertEqual(self.system.switches, [NEW])
        self.assertEqual(self.system.running, NEW)
        self.assertEqual((self.state["deployed"], self.state["floor"]), (REV, 7))
        self.assertIsNone(self.state["switching"])
        # Persisted before the profile changed, so a crash cannot skip verification.
        # No package was deployed before: going back means this host's own.
        self.assertEqual(self.saves[-1]["switching"], {"key": "7.1", "rev": REV, "previous": None})
        self.assertEqual(self.system.discarded, 1)
        self.assertIn(f"DEPLOY succeeded rev={REV} path={NEW}", self.output.getvalue())
        for state in (self.state, self.saves[-1]):
            deploy.validate_state(json.loads(json.dumps(state)))

    def test_first_look_records_the_latest_run_and_deploys_only_later_ones(self):
        # A host that has just enabled deployment, with a run from before it did.
        self.state = deploy.new_state()
        github = FakeGitHub([workflow_run()])
        self.assertEqual(self.tick(github), "baseline recorded")
        self.assertEqual(self.state["floor"], 7)
        self.assertEqual(self.tick(github), "up to date")
        # Another attempt of that run is still the request from before.
        github.runs = [workflow_run(run_attempt=2)]
        self.assertEqual(self.tick(github), "up to date")
        self.assertEqual((self.system.built, self.state["failed"]), ([], []))
        self.assertFalse(any("/jobs" in path for path in github.requests))
        # A run dispatched since is deployed, of the same commit or another.
        github.runs = [workflow_run(id=8)]
        self.assertEqual(self.tick(github), "deployed")
        self.assertEqual((self.state["deployed"], self.state["floor"]), (REV, 8))

    def test_first_look_without_any_run_lets_the_first_run_deploy(self):
        self.state = deploy.new_state()
        github = FakeGitHub([])
        self.assertEqual(self.tick(github), "baseline recorded")
        self.assertEqual(self.state["floor"], 0)
        self.assertEqual(self.tick(github), "no successful run")
        github.runs = [workflow_run()]
        self.assertEqual(self.tick(github), "deployed")

    def test_older_run_is_refused_when_a_later_one_is_gone(self):
        github = FakeGitHub([workflow_run(id=7)])
        self.assertEqual(self.tick(github), "deployed")
        github.runs, github.jobs = [workflow_run(id=8, head_sha=LATER)], [job(head_sha=LATER)]
        self.system.result = NEWER
        self.assertEqual(self.tick(github), "deployed")
        # Run 8 is deleted on GitHub: the latest successful run on main is 7 again.
        github.runs, github.jobs = [workflow_run(id=7)], [job()]
        self.system.result = NEW
        self.assertEqual(self.tick(github), "refused")
        self.assertEqual((self.state["deployed"], self.state["floor"], self.system.running), (LATER, 8, NEWER))
        self.assertEqual((self.system.built, self.system.switches), ([REV, LATER], [NEW, NEWER]))
        self.assertIn("DEPLOY FAILED run=7: older than run 8", self.output.getvalue())
        self.assertEqual(self.tick(github), "up to date")
        self.assertEqual(self.output.getvalue().count("DEPLOY FAILED"), 1)

    def test_commit_must_be_the_head_of_main_or_one_of_its_ancestors(self):
        # A run dispatched on a tag named main reports the branch name "main" as well.
        github = FakeGitHub([workflow_run()])
        github.moved = LATER
        self.assertEqual(self.tick(github), "refused")
        self.assertEqual((self.system.built, self.state["failed"]), ([], ["7.1"]))
        self.assertIn(f"DEPLOY FAILED rev={REV} run=7: the commit is not on main", self.output.getvalue())
        self.assertEqual(self.tick(github), "up to date")
        # main moved on while the run was building: its commit is still on main.
        github.runs = [workflow_run(id=8)]
        github.ancestors = {REV}
        self.assertEqual(self.tick(github), "deployed")
        self.assertIn(f"/repos/bitemyapp/sekka/compare/{LATER}...{REV}", github.requests)

    def test_unchanged_runs_list_and_deployed_revision_do_nothing(self):
        github = FakeGitHub([workflow_run()])
        self.tick(github)
        self.assertEqual(self.tick(github), "up to date")
        self.assertEqual(github.unchanged, 1)
        self.assertEqual(self.system.built, [REV])
        self.assertEqual(self.system.switches, [NEW])

    def test_unhealthy_bot_is_rolled_back_and_that_attempt_never_retried(self):
        self.system.broken = {NEW}
        github = FakeGitHub([workflow_run()])
        with self.assertRaises(deploy.DeployError):
            self.tick(github)
        # Nothing was deployed before, so no profile is left to outrank this host's package.
        self.assertEqual(self.system.switches, [NEW])
        self.assertIsNone(self.system.profile)
        self.assertEqual(self.system.running, OLD)
        self.assertIsNone(self.state["deployed"])
        self.assertEqual((self.state["failed"], self.state["floor"]), (["7.1"], 0))
        self.assertIn(f"DEPLOY FAILED rev={REV}: health checks did not pass; restored this host's package",
                      self.output.getvalue())
        self.assertEqual(self.tick(github), "up to date")
        self.assertEqual(self.system.built, [REV])
        # Re-running the workflow is how an operator asks for another attempt.
        github.runs = [workflow_run(run_attempt=2)]
        self.system.broken = set()
        self.assertEqual(self.tick(github), "deployed")

    def test_unhealthy_bot_is_rolled_back_to_the_package_deployed_before(self):
        github = FakeGitHub([workflow_run()])
        self.assertEqual(self.tick(github), "deployed")
        github.runs, github.jobs = [workflow_run(id=8, head_sha=LATER)], [job(head_sha=LATER)]
        self.system.result = NEWER
        self.system.broken = {NEWER}
        with self.assertRaises(deploy.DeployError):
            self.tick(github)
        self.assertEqual(self.system.switches, [NEW, NEWER, NEW])
        self.assertEqual((self.system.profile, self.system.running), (NEW, NEW))
        self.assertEqual((self.state["deployed"], self.state["floor"]), (REV, 7))
        self.assertIn(f"restored {NEW}", self.output.getvalue())

    def test_monitor_regression_rolls_back(self):
        self.system.broken_monitor = {NEW}
        with self.assertRaises(deploy.DeployError):
            self.tick(FakeGitHub([workflow_run()]))
        self.assertEqual(self.system.switches, [NEW])
        self.assertIsNone(self.system.profile)
        self.assertEqual(self.state["failed"], ["7.1"])

    def test_monitor_already_failing_is_not_blamed_on_the_deploy(self):
        self.system.broken_monitor = {OLD, NEW}
        self.assertEqual(self.tick(FakeGitHub([workflow_run()])), "deployed")

    def test_run_whose_job_was_skipped_or_missing_is_not_deployed(self):
        for jobs in ([job(conclusion="skipped")], [], [job(head_sha="b" * 40)]):
            self.state = listening_state()
            self.assertEqual(self.tick(FakeGitHub([workflow_run()], jobs)), "skipped")
            self.assertEqual(self.state["failed"], ["7.1"])
        self.assertEqual(self.system.built, [])

    def test_build_failures_retry_with_backoff_then_give_up(self):
        self.system.result = deploy.BuildError("Build failed")
        github = FakeGitHub([workflow_run()])
        with self.assertRaises(deploy.BuildError):
            self.tick(github, 1000)
        self.assertEqual(self.state["next_attempt"], 1000 + deploy.RETRY)
        self.assertEqual(self.tick(github, 1000 + deploy.RETRY - 1), "waiting to retry")
        for attempt in (1, 2):
            with self.assertRaises(deploy.BuildError):
                self.tick(github, 1000 + attempt * deploy.RETRY)
        self.assertEqual(self.state["failed"], ["7.1"])
        self.assertIsNone(self.state["retry"])
        self.assertEqual(len(self.system.built), deploy.BUILD_ATTEMPTS)
        self.assertIn(f"build failed {deploy.BUILD_ATTEMPTS} times", self.output.getvalue())
        self.assertEqual(self.system.discarded, deploy.BUILD_ATTEMPTS)

    def test_package_without_yuki_programs_is_not_retried(self):
        self.system.result = deploy.DeployError("Built package lacks Yuki's programs")
        with self.assertRaises(deploy.DeployError):
            self.tick(FakeGitHub([workflow_run()]))
        self.assertEqual(self.state["failed"], ["7.1"])
        self.assertEqual(self.system.switches, [])

    def test_identical_build_records_revision_without_restart(self):
        self.system.result = OLD
        self.assertEqual(self.tick(FakeGitHub([workflow_run()])), "unchanged")
        self.assertEqual(self.system.switches, [])
        self.assertEqual(self.system.invocation, 1)
        self.assertEqual((self.state["deployed"], self.state["floor"]), (REV, 7))

    def test_interrupted_deployment_is_rolled_back_by_the_next_tick(self):
        self.state["switching"] = {"key": "7.1", "rev": REV, "previous": NEW}
        self.system.profile = self.system.running = NEWER
        self.assertEqual(self.tick(FakeGitHub([workflow_run()])), "up to date")
        self.assertEqual(self.system.switches, [NEW])
        self.assertEqual(self.system.running, NEW)
        self.assertEqual(self.state["failed"], ["7.1"])
        self.assertIsNone(self.saves[0]["switching"])

    def test_interrupted_first_deployment_leaves_this_hosts_package(self):
        self.state["switching"] = {"key": "7.1", "rev": REV, "previous": None}
        self.system.profile = self.system.running = NEW
        self.assertEqual(self.tick(FakeGitHub([workflow_run()])), "up to date")
        self.assertEqual(self.system.switches, [])
        self.assertIsNone(self.system.profile)
        self.assertEqual(self.system.running, OLD)
        self.assertIn("rolled back to this host's package", self.output.getvalue())

    def test_rate_limit_defers_every_request(self):
        github = FakeGitHub([workflow_run()])
        github.get = mock.Mock(side_effect=deploy.RateLimited(5000))
        self.assertEqual(self.tick(github, 1000), "waiting for the GitHub API rate limit")
        self.assertEqual(self.tick(github, 4999), "waiting to retry")
        github.get.assert_called_once()

    def test_runs_must_match_repository_branch_workflow_event_and_success(self):
        for fields in ({"head_branch": "feature"}, {"event": "push"}, {"path": ".github/workflows/nightly.yml"},
                       {"head_repository": {"full_name": "attacker/sekka"}}, {"head_repository": None},
                       {"head_sha": "not-a-commit"}, {"conclusion": "failure"}, {"id": "7"}):
            with self.assertRaises(deploy.DeployError, msg=str(fields)):
                deploy.parse_run(workflow_run(**fields), "bitemyapp/sekka", "yuki.yml", "main")
        self.assertEqual(deploy.parse_run(workflow_run(), "bitemyapp/sekka", "yuki.yml", "main"),
                         {"id": 7, "attempt": 1, "sha": REV})


class GitHubTests(unittest.TestCase):
    def response(self, status, body=b"{}", **headers):
        response = mock.Mock(status=status)
        response.read.return_value = body
        response.getheader.side_effect = lambda name: headers.get(name.replace("-", "_"))
        connection = mock.Mock()
        connection.getresponse.return_value = response
        return mock.patch.object(deploy.http.client, "HTTPSConnection", return_value=connection), connection

    def test_conditional_request_reuses_etag(self):
        patcher, connection = self.response(304)
        with patcher as constructor:
            self.assertEqual(deploy.GitHub("https://api.github.com", "bitemyapp/sekka").get("/x", '"tag"'), (None, '"tag"'))
        self.assertEqual(connection.request.call_args.kwargs["headers"]["If-None-Match"], '"tag"')
        self.assertEqual(constructor.call_args.args[:2], ("api.github.com", 443))

    def test_rate_limit_waits_until_reset_within_bounds(self):
        patcher, _ = self.response(403, x_ratelimit_remaining="0", x_ratelimit_reset="1000500")
        with patcher, mock.patch.object(deploy.time, "time", return_value=1000000):
            with self.assertRaises(deploy.RateLimited) as error:
                deploy.GitHub("https://api.github.com", "bitemyapp/sekka").get("/x")
        self.assertEqual(error.exception.until, 1000500)

    def test_api_must_be_an_https_origin(self):
        for api in ("http://api.github.com", "https://api.github.com/proxy", "https://"):
            with self.assertRaises(deploy.DeployError):
                deploy.GitHub(api, "bitemyapp/sekka")

    def test_rejected_and_oversized_responses_are_errors(self):
        github = deploy.GitHub("https://api.github.com", "bitemyapp/sekka")
        patcher, _ = self.response(404)
        with patcher, self.assertRaises(deploy.DeployError) as error:
            github.get("/x")
        self.assertNotIsInstance(error.exception, deploy.TooLarge)
        patcher, _ = self.response(200, b" " * (deploy.MAX_HTTP + 1))
        with patcher, self.assertRaises(deploy.TooLarge):
            github.get("/x")

    def test_branch_head_is_the_branch_reference_and_nothing_else(self):
        github = deploy.GitHub("https://api.github.com", "bitemyapp/sekka")
        commit = {"type": "commit", "sha": REV}
        with mock.patch.object(github, "get", return_value=({"ref": "refs/heads/main", "object": commit}, None)) as get:
            self.assertEqual(github.branch_head("main"), REV)
        get.assert_called_once_with("/repos/bitemyapp/sekka/git/ref/heads/main")
        for body in ({"ref": "refs/tags/main", "object": commit}, {"ref": "refs/heads/maintenance", "object": commit},
                     {"ref": "refs/heads/main", "object": {"type": "tag", "sha": REV}},
                     {"ref": "refs/heads/main", "object": {"type": "commit", "sha": "main"}},
                     [{"ref": "refs/heads/main", "object": commit}]):
            with mock.patch.object(github, "get", return_value=(body, None)):
                with self.assertRaises(deploy.DeployError, msg=str(body)):
                    github.branch_head("main")

    def test_commit_is_on_the_branch_only_as_its_head_or_strictly_behind_it(self):
        github = deploy.GitHub("https://api.github.com", "bitemyapp/sekka")

        def api(comparison):
            def get(path, etag=None):
                if "/git/ref/heads/" in path:
                    return {"ref": "refs/heads/main", "object": {"type": "commit", "sha": LATER}}, None
                self.assertEqual(path, f"/repos/bitemyapp/sekka/compare/{LATER}...{REV}")
                if isinstance(comparison, Exception):
                    raise comparison
                return comparison, None
            return mock.patch.object(github, "get", side_effect=get)

        with api(None) as get:
            self.assertIs(github.on_branch(LATER, "main"), True)
        get.assert_called_once()
        for comparison, expected in (
                ({"status": "behind", "ahead_by": 0, "merge_base_commit": {"sha": REV}}, True),
                ({"status": "diverged", "ahead_by": 1, "merge_base_commit": {"sha": "c" * 40}}, False),
                ({"status": "ahead", "ahead_by": 2, "merge_base_commit": {"sha": LATER}}, False),
                ({"status": "behind", "ahead_by": 0, "merge_base_commit": {"sha": "c" * 40}}, False),
                ({"status": "behind", "ahead_by": 1, "merge_base_commit": {"sha": REV}}, False),
                ({"status": "identical", "ahead_by": 0, "merge_base_commit": {"sha": REV}}, False),
                # A commit that is not behind the head comes with its changes, of any size.
                (deploy.TooLarge("GitHub API response exceeds its size limit"), False)):
            with api(comparison):
                self.assertIs(github.on_branch(REV, "main"), expected, msg=str(comparison))
        with api({"status": "behind"}), self.assertRaises(deploy.DeployError):
            github.on_branch(REV, "main")


class SystemTests(unittest.TestCase):
    def test_restart_waits_for_a_new_invocation_and_steady_health(self):
        system = FakeSystem()
        system.profile = NEW
        self.assertTrue(deploy.restart(system))
        self.assertEqual(system.running, NEW)
        system.broken = {NEW}
        self.assertFalse(deploy.restart(system))

    def test_switch_keeps_recent_generations_and_the_current_one(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(os.path.realpath(directory))
            package = root / "package"
            package.mkdir()
            for number in range(1, 9):
                (root / f"profile-{number}-link").symlink_to(package)

            def nix(args, timeout):
                (root / "profile-9-link").symlink_to(package)
                (root / "profile").symlink_to("profile-9-link")
                return 0, b""

            system = deploy.System(root, OLD, "github:bitemyapp/sekka/{rev}#sekka-discord")
            with mock.patch.object(system, "run", side_effect=nix) as run:
                system.switch(str(package))
            self.assertEqual(run.call_args.args[0][-4:], ["--no-link", "--profile", str(root / "profile"), str(package)])
            self.assertEqual(sorted(p.name for p in root.glob("profile-*-link")),
                             [f"profile-{n}-link" for n in range(5, 10)])

    def test_restore_returns_to_a_deployed_package_or_removes_the_profile(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(os.path.realpath(directory))
            package = root / "package"
            package.mkdir()
            for number in (1, 2):
                (root / f"profile-{number}-link").symlink_to(package)
            (root / "profile").symlink_to("profile-2-link")
            (root / "state.json").touch()
            system = deploy.System(root, OLD, "github:bitemyapp/sekka/{rev}#sekka-discord")
            with mock.patch.object(deploy, "STORE_PATH", re.compile(re.escape(str(package)))):
                self.assertEqual((system.deployed(), system.current()), (str(package), str(package)))
                with mock.patch.object(system, "switch") as switch:
                    system.restore(str(package))
                switch.assert_called_once_with(str(package))
                self.assertTrue((root / "profile").is_symlink())
                # With no earlier deployment, nothing may stay that Yuki's launchers would prefer.
                system.restore(None)
                self.assertEqual(sorted(entry.name for entry in root.iterdir()), ["package", "state.json"])
                self.assertEqual((system.deployed(), system.current()), (None, OLD))
                system.restore(None)

    def test_build_requires_one_store_path_with_both_programs(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(os.path.realpath(directory))
            package = root / "package"
            (package / "bin").mkdir(parents=True)
            (package / "bin" / "sekka-discord").touch(mode=0o755)
            (root / "candidate").symlink_to(package)
            system = deploy.System(root, OLD, "github:bitemyapp/sekka/{rev}#sekka-discord")
            with mock.patch.object(deploy, "STORE_PATH", re.compile(re.escape(str(package)))), \
                    mock.patch.object(system, "run", return_value=(0, str(package).encode() + b"\n")) as run:
                with self.assertRaises(deploy.DeployError) as error:
                    system.build(REV)
                self.assertNotIsInstance(error.exception, deploy.BuildError)
                (package / "bin" / "sekka-discord-monitor").touch(mode=0o755)
                self.assertEqual(system.build(REV), str(package))
            self.assertEqual(run.call_args.args[0][-1], f"github:bitemyapp/sekka/{REV}#sekka-discord")
            self.assertIn("accept-flake-config", run.call_args.args[0])
            with mock.patch.object(system, "run", return_value=(1, b"")):
                with self.assertRaises(deploy.BuildError):
                    system.build(REV)

    def test_state_is_private_atomic_locked_and_validated(self):
        with tempfile.TemporaryDirectory() as directory:
            os.chmod(directory, 0o755)
            state = deploy.new_state()
            state["failed"] = ["7.1"]
            with deploy.Store(directory) as store:
                store.save(state)
                self.assertEqual(store.load(), state)
                self.assertEqual(stat.S_IMODE((Path(directory) / "state.json").stat().st_mode), 0o600)
                with self.assertRaises(deploy.DeployError):
                    with deploy.Store(directory):
                        self.fail("second process lock should not be acquired")
                (Path(directory) / "state.json").write_text('{"version": 1, "failed": "everything"}')
                with self.assertRaises(deploy.DeployError):
                    store.load()

    def test_state_file_must_be_private_and_earlier_state_gains_a_floor(self):
        with tempfile.TemporaryDirectory() as directory:
            os.chmod(directory, 0o755)
            path = Path(directory) / "state.json"
            earlier = deploy.new_state()
            del earlier["floor"]
            with deploy.Store(directory) as store:
                path.write_text(json.dumps(earlier))
                path.chmod(0o600)
                self.assertEqual(store.load(), deploy.new_state())
                for mode in (0o640, 0o604):
                    path.chmod(mode)
                    with self.assertRaises(deploy.DeployError, msg=oct(mode)):
                        store.load()
                for floor in (-1, True, "7", 7.0):
                    path.write_text(json.dumps({**deploy.new_state(), "floor": floor}))
                    path.chmod(0o600)
                    with self.assertRaises(deploy.DeployError, msg=repr(floor)):
                        store.load()

    def test_missing_settings_fail_before_any_request(self):
        with mock.patch.dict(os.environ, {"SEKKA_DEPLOY_REPOSITORY": "bitemyapp/sekka"}, clear=True), \
                mock.patch.object(deploy, "GitHub") as github, mock.patch("sys.stderr", new=io.StringIO()):
            self.assertEqual(deploy.main([]), 1)
        github.assert_not_called()


if __name__ == "__main__":
    unittest.main()
