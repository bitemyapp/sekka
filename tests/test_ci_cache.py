"""Retention boundaries, the wrapper itself, and real Git object reuse with
private candidate state."""
import fcntl
import importlib.util
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest


SCRIPTS = Path(__file__).resolve().parents[1] / "scripts"
spec = importlib.util.spec_from_file_location("retain", SCRIPTS / "ci-retain.py")
retain = importlib.util.module_from_spec(spec)
spec.loader.exec_module(retain)
PLENTY = retain.MIN_FREE * 2


class Retention(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)
        self.now = 2000000000

    def tearDown(self):
        self.tmp.cleanup()

    def generation(self, group, age, success=True):
        path = self.root / group / f"{self.now - age}-fixture"
        path.mkdir(parents=True)
        if success:
            (path / "success").touch()
        return path

    def test_success_failure_age_and_count_bounds(self):
        main = [self.generation("main-checks", age) for age in (1, 2, 3, 8 * retain.DAY)]
        prs = [self.generation("pr-checks", age) for age in (1, 2, 3, 4, 5, 8 * retain.DAY)]
        failed = [self.generation("main-checks", age, False) for age in (10, 11, 2 * retain.DAY)]
        failed_prs = [self.generation("pr-checks", age, False) for age in (10, 11, 12, 2 * retain.DAY)]
        old_main = self.generation("main-uta", 30 * retain.DAY)
        old_pr = self.generation("pr-uta", 30 * retain.DAY)
        unrelated = self.generation("unrelated", 30 * retain.DAY)
        retain.prune(self.root, self.now, PLENTY)
        self.assertEqual([p.exists() for p in main], [True, True, False, False])
        self.assertEqual([p.exists() for p in prs], [True, True, True, True, False, False])
        self.assertEqual([p.exists() for p in failed], [True, False, False])
        self.assertEqual([p.exists() for p in failed_prs], [True, True, False, False])
        self.assertTrue(old_main.exists())
        self.assertFalse(old_pr.exists())
        self.assertTrue(unrelated.exists())

    def test_low_free_space_keeps_only_the_newest_main_success(self):
        main = [self.generation("main-checks", age) for age in (1, 2)]
        failed = self.generation("main-checks", 0, False)
        prs = [self.generation("pr-checks", age) for age in (1, 2)]
        retain.prune(self.root, self.now, retain.MIN_FREE - 1)
        self.assertEqual([p.exists() for p in main], [True, False])
        self.assertFalse(failed.exists())
        self.assertEqual([p.exists() for p in prs], [False, False])

    def test_a_generation_being_built_is_left_alone(self):
        older = [self.generation("pr-checks", age) for age in (1, 2, 3, 4)]
        running = self.generation("pr-checks", 30 * retain.DAY, False)
        fd = retain.open_lock(running / ".building")
        fcntl.flock(fd, fcntl.LOCK_EX)
        try:
            retain.prune(self.root, self.now, 0)
            self.assertTrue(running.exists())
            self.assertEqual([p.exists() for p in older], [False] * 4)
        finally:
            os.close(fd)
        retain.prune(self.root, self.now, PLENTY)
        self.assertFalse(running.exists())

    def test_queued_consumers_survive_history_pressure_until_release_or_expiry(self):
        queued = self.generation("pr-build-clients", 10)
        retain.lease_consumers(queued, "291-1", self.now)
        newer = [self.generation("pr-build-clients", age) for age in (1, 2, 3, 4, 5)]
        retain.prune(self.root, self.now, PLENTY)
        self.assertTrue(queued.exists())
        self.assertEqual([p.exists() for p in newer], [True, True, True, True, False])
        other = self.generation("pr-build-clients", 20)
        retain.lease_consumers(other, "292-1", self.now)
        retain.prune(self.root, self.now, 0)
        self.assertTrue(queued.exists())
        self.assertTrue(other.exists())
        retain.release_consumers(self.root, "291-1")
        retain.prune(self.root, self.now, 0)
        self.assertFalse(queued.exists())
        self.assertTrue(other.exists())
        retain.prune(self.root, self.now + retain.CONSUMER_LEASE_SECONDS + 1, 0)
        self.assertFalse(other.exists())

    def test_pruning_does_not_follow_symlink_directories(self):
        root = self.root / "roots"
        outside = self.root / "outside"
        root.mkdir()
        (outside / "1-old").mkdir(parents=True)
        (root / "main-checks").symlink_to(outside, target_is_directory=True)
        (root / "pr-checks").mkdir()
        (root / "pr-checks/1-old").symlink_to(outside, target_is_directory=True)
        retain.prune(root, self.now, 0)
        self.assertTrue((outside / "1-old").exists())

    def test_locks_refuse_fifos_and_links_without_blocking(self):
        os.mkfifo(self.root / "fifo")
        with self.assertRaises(OSError):
            retain.open_lock(self.root / "fifo")
        (self.root / "link").symlink_to(self.root / "target")
        with self.assertRaises(OSError):
            retain.open_lock(self.root / "link")
        self.assertFalse((self.root / "target").exists())


class Wrapper(unittest.TestCase):
    """The wrapper with stand-ins for `nix` and `nix-store` on PATH."""

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        base = Path(self.tmp.name)
        self.roots = base / "roots"
        self.roots.mkdir()
        self.args = base / "nix-args"
        # Any real store path stands in for a build result and its input.
        self.store_path = str(Path(sys.executable).resolve().parents[1])
        if not retain.STORE_PATH.fullmatch(self.store_path):
            self.skipTest("needs Python from the Nix store")
        bin_dir = base / "bin"
        bin_dir.mkdir()
        bash = shutil.which("bash")
        (bin_dir / "nix").write_text(f"""#!{bash}
printf '%s\\n' "$@" > {self.args}
while (($#)); do [[ $1 == --out-link ]] && ln -s {self.store_path} "$2"; shift; done
exit "${{FAKE_STATUS:-0}}"
""")
        (bin_dir / "nix-store").write_text(f"""#!{bash}
[[ -z ${{FAKE_QUERY_FAILS:-}} ]] || exit 1
echo {self.store_path}
""")
        for tool in ("nix", "nix-store"):
            (bin_dir / tool).chmod(0o755)
        self.env = dict(os.environ, PATH=f"{bin_dir}:{os.environ['PATH']}",
                        SEKKA_CI_ROOTS=str(self.roots), SEKKA_CI_MIN_FREE_BYTES="0")

    def tearDown(self):
        self.tmp.cleanup()

    def run_retain(self, *args, **env):
        return subprocess.run([sys.executable, str(SCRIPTS / "ci-retain.py"), *args],
                              env=dict(self.env, **env), capture_output=True, text=True)

    def generations(self, group):
        return sorted((self.roots / group).iterdir())

    def test_success_roots_results_and_inputs(self):
        result = self.run_retain("pr", "checks", "flake", "check", "--keep-going")
        self.assertEqual(result.returncode, 0, result.stderr)
        [generation] = self.generations("pr-checks")
        self.assertTrue((generation / "success").is_file())
        self.assertEqual(os.readlink(generation / "result"), self.store_path)
        name = Path(self.store_path).name
        self.assertEqual(os.readlink(generation / "dependencies" / name), self.store_path)
        self.assertFalse(retain.building(generation))
        self.assertEqual(self.args.read_text().split("\n")[:3], ["flake", "check", "--keep-going"])

    def test_failure_keeps_its_status_and_passing_results(self):
        result = self.run_retain("main", "uta", "build", ".#uta", FAKE_STATUS="23")
        self.assertEqual(result.returncode, 23)
        [generation] = self.generations("main-uta")
        self.assertFalse((generation / "success").exists())
        self.assertTrue((generation / "result").is_symlink())

    def test_an_input_query_failure_does_not_fail_a_passing_build(self):
        result = self.run_retain("main", "packages", "build", ".#tools", FAKE_QUERY_FAILS="1")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("::warning::", result.stderr)
        [generation] = self.generations("main-packages")
        self.assertTrue((generation / "success").is_file())

    def test_without_mounted_roots_it_runs_plain_nix(self):
        env = dict(self.env)
        del env["SEKKA_CI_ROOTS"]
        self.env = env
        result = self.run_retain("pr", "uta", "build", ".#uta")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.args.read_text().split(), ["build", ".#uta", "--no-link"])
        result = self.run_retain("pr", "checks", "flake", "check")
        self.assertEqual(self.args.read_text().split(), ["flake", "check"])
        self.assertEqual(self.run_retain("prune").returncode, 0)
        self.assertEqual(list(self.roots.iterdir()), [])

    def test_it_owns_the_out_link(self):
        result = self.run_retain("pr", "uta", "build", ".#uta", "--out-link", "elsewhere")
        self.assertEqual(result.returncode, 2)
        self.assertFalse(self.args.exists())

    def test_client_handoff_is_leased_before_immediate_low_space_prune(self):
        env = dict(GITHUB_RUN_ID="291", GITHUB_RUN_ATTEMPT="2",
                   SEKKA_CI_MIN_FREE_BYTES=str(2**62))
        result = self.run_retain("pr", "build-clients", "build", ".#shared-build-inputs", **env)
        self.assertEqual(result.returncode, 0, result.stderr)
        [generation] = self.generations("pr-build-clients")
        self.assertTrue((generation / "result").is_symlink())
        wrong_run = self.run_retain("release-consumers", **dict(env, GITHUB_RUN_ID="292"))
        self.assertEqual(wrong_run.returncode, 0, wrong_run.stderr)
        self.assertTrue(generation.exists())
        released = self.run_retain("release-consumers", **env)
        self.assertEqual(released.returncode, 0, released.stderr)
        self.assertFalse(generation.exists())


class GitCache(unittest.TestCase):
    def test_clean_candidates_reuse_objects_without_sharing_writable_state(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            env = dict(os.environ, HOME=str(root), GIT_CONFIG_NOSYSTEM="1",
                       SEKKA_CI_GIT_CACHE=str(root / "cache"))

            def run(*args, cwd=root):
                return subprocess.check_output(args, cwd=cwd, env=env, text=True).strip()

            upstream = root / "upstream"
            run("git", "init", "--initial-branch=main", str(upstream))
            run("git", "config", "--global", "user.email", "ci@example.invalid")
            run("git", "config", "--global", "user.name", "CI test")
            run("git", "config", "--global", f"url.{upstream}.insteadOf", "https://github.com/bitemyapp/sekka")
            (upstream / "scripts").mkdir()
            (upstream / "scripts/check-pr-trust.py").write_text("trusted main gate\n")
            (upstream / "payload").write_text("main payload\n")
            (upstream / "flake.nix").write_text("{}\n")
            (upstream / "nested/source").mkdir(parents=True)
            (upstream / "nested/source/file").write_text("nested payload\n")
            run("git", "add", ".", cwd=upstream)
            run("git", "commit", "-m", "main", cwd=upstream)
            main = run("git", "rev-parse", "HEAD", cwd=upstream)
            run("bash", str(SCRIPTS / "runner-git-cache.sh"), "refresh")
            cache = root / "cache/repository.git"
            run("git", "checkout", "-b", "candidate", cwd=upstream)
            (upstream / "payload").write_text("candidate payload\n")
            (upstream / "scripts/check-pr-trust.py").write_text("candidate gate must not run\n")
            run("git", "commit", "-am", "candidate", cwd=upstream)
            candidate = run("git", "rev-parse", "HEAD", cwd=upstream)
            for number in range(2):
                workspace = root / f"job-{number}"
                env["GITHUB_WORKSPACE"] = str(workspace)
                run("bash", str(SCRIPTS / "runner-git-cache.sh"), "seed")
                self.assertIn(str(cache / "objects"), (workspace / ".git/objects/info/alternates").read_text())
                # The sequence checkout uses before fetching the immutable ref.
                run("git", "checkout", "--detach", cwd=workspace)
                run("git", "clean", "-ffdx", cwd=workspace)
                run("git", "reset", "--hard", "HEAD", cwd=workspace)
                self.assertFalse((workspace / "payload").exists())
                self.assertEqual((workspace / "scripts/check-pr-trust.py").read_text(), "trusted main gate\n")
                # checkout v6's non-cone sparse setup writes the repository
                # config, unlike the seed's sparse-checkout CLI, which uses
                # config.worktree. Exercise both scopes, as the PR job does.
                run("git", "config", "core.sparseCheckout", "true", cwd=workspace)
                # Only after the immutable gate admits this exact SHA does
                # checkout fetch and expand the candidate's source.
                run("git", "fetch", "--no-tags", "--depth=1", "origin", candidate, cwd=workspace)
                # The second checkout explicitly selects every path. Its
                # default sparse-disable path instead unsets worktreeConfig,
                # exposing the first action's core.sparseCheckout=true and
                # applying the old narrow patterns again at checkout.
                (workspace / ".git/info/sparse-checkout").write_text("/*\n")
                run("git", "checkout", "--detach", "--force", candidate, cwd=workspace)
                self.assertEqual(run("git", "rev-parse", "HEAD", cwd=workspace), candidate)
                self.assertEqual((workspace / "payload").read_text(), "candidate payload\n")
                self.assertEqual((workspace / "flake.nix").read_text(), "{}\n")
                self.assertEqual((workspace / "nested/source/file").read_text(), "nested payload\n")
                (workspace / "payload").write_text("candidate mutation\n")
                (workspace / "untracked").touch()
                run("git", "add", ".", cwd=workspace)
                run("git", "commit", "-m", "private job state", cwd=workspace)
            self.assertEqual(run("git", "--git-dir", str(cache), "rev-parse", "main"), main)
            self.assertEqual(run("git", "--git-dir", str(cache), "show", "main:payload"), "main payload")


if __name__ == "__main__":
    unittest.main()
