"""Retention boundaries and real Git object reuse with private candidate state."""
import importlib.util
import os
from pathlib import Path
import subprocess
import tempfile
import unittest


SCRIPTS = Path(__file__).resolve().parents[1] / "scripts"
spec = importlib.util.spec_from_file_location("retain", SCRIPTS / "ci-retain.py")
retain = importlib.util.module_from_spec(spec)
spec.loader.exec_module(retain)


class Retention(unittest.TestCase):
    def test_success_failure_age_and_count_bounds(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            now = 2000000000

            def generation(group, age, success=True):
                path = root / group / f"{now - age}-fixture"
                path.mkdir(parents=True)
                if success:
                    (path / "success").touch()
                return path

            main = [generation("main-checks", age) for age in (1, 2, 3, 8 * retain.DAY)]
            prs = [generation("pr-checks", age) for age in (1, 2, 3, 4, 5, 8 * retain.DAY)]
            failed = [generation("main-checks", age, False) for age in (10, 11, 2 * retain.DAY)]
            old_main = generation("main-uta", 30 * retain.DAY)
            old_pr = generation("pr-uta", 30 * retain.DAY)
            unrelated = generation("unrelated", 30 * retain.DAY)
            retain.prune(root, now)
            self.assertEqual([p.exists() for p in main], [True, True, False, False])
            self.assertEqual([p.exists() for p in prs], [True, True, True, True, False, False])
            self.assertEqual([p.exists() for p in failed], [True, False, False])
            self.assertTrue(old_main.exists())
            self.assertFalse(old_pr.exists())
            self.assertTrue(unrelated.exists())

    def test_pruning_does_not_follow_symlink_directories(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp) / "roots"
            outside = Path(tmp) / "outside"
            root.mkdir()
            (outside / "1-old").mkdir(parents=True)
            (root / "main-checks").symlink_to(outside, target_is_directory=True)
            (root / "pr-checks").mkdir()
            (root / "pr-checks/1-old").symlink_to(outside, target_is_directory=True)
            retain.prune(root)
            self.assertTrue((outside / "1-old").exists())


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
                # Only after the immutable gate admits this exact SHA does
                # checkout fetch and expand the candidate's source.
                run("git", "fetch", "--no-tags", "--depth=1", "origin", candidate, cwd=workspace)
                run("git", "sparse-checkout", "disable", cwd=workspace)
                run("git", "checkout", "--detach", candidate, cwd=workspace)
                self.assertEqual(run("git", "rev-parse", "HEAD", cwd=workspace), candidate)
                self.assertEqual((workspace / "payload").read_text(), "candidate payload\n")
                (workspace / "payload").write_text("candidate mutation\n")
                (workspace / "untracked").touch()
                run("git", "add", ".", cwd=workspace)
                run("git", "commit", "-m", "private job state", cwd=workspace)
            self.assertEqual(run("git", "--git-dir", str(cache), "rev-parse", "main"), main)
            self.assertEqual(run("git", "--git-dir", str(cache), "show", "main:payload"), "main payload")


if __name__ == "__main__":
    unittest.main()
