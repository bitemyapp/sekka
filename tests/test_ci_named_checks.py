"""ci-named-checks.py with stand-ins for `nix` and `nix-store`: results read
from the store's own record, each failure named, its log apart from the
others'."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


SCRIPTS = Path(__file__).resolve().parents[1] / "scripts"
HASH = "0123456789abcdfghijklmnpqrsvwxyz"

# `nix eval` prints the fixture. `nix build` records its arguments and prints
# log lines for each derivation it has to build; one the fixture lets pass
# becomes valid, and a failing one leaves its output directory behind, as
# Nix does. `nix-store --check-validity --print-invalid` lists the outputs
# that are not valid.
FAKE_NIX = """#!{python}
import json, os, sys
fixture = json.load(open({fixture!r}))
valid = set(open({valid!r}).read().split())
if os.path.basename(sys.argv[0]) == "nix-store":
    assert sys.argv[1:3] == ["--check-validity", "--print-invalid"], sys.argv
    print("\\n".join(path for path in sys.argv[3:] if path not in valid))
    sys.exit(0)
if sys.argv[1] == "eval":
    if fixture.get("eval_fails"):
        sys.exit("error: attribute missing")
    print(json.dumps(fixture["checks"]))
    sys.exit(0)
with open({calls!r}, "a") as f:
    f.write(json.dumps(sys.argv[1:]) + "\\n")
status = 0
for arg in sys.argv[2:]:
    if not arg.endswith(".drv^out"):
        continue
    name = os.path.basename(arg)[33:-len(".drv^out")]
    [check] = [c for c in fixture["checks"].values() if c["drv"] + "^out" == arg]
    if check["out"] in valid:
        continue
    print(name + "> starting " + name, file=sys.stderr)
    print("\\x1b[2m" + name + "-dependency> not the check's own line\\x1b[0m", file=sys.stderr)
    os.makedirs(check["out"], exist_ok=True)
    if name in fixture["failing"]:
        print(name + "> \\x1b[31massertion failed\\x1b[0m <in> `" + name + "`", file=sys.stderr)
        status = 1
    else:
        print(name + "> finished", file=sys.stderr)
        with open({valid!r}, "a") as f:
            f.write(check["out"] + "\\n")
if status:
    print("error: build of some derivations failed", file=sys.stderr)
sys.exit(status)
"""


class NamedChecks(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        base = Path(self.tmp.name)
        self.outputs = base / "store"
        self.outputs.mkdir()
        self.fixture = base / "fixture.json"
        self.calls = base / "calls"
        self.summary = base / "summary.md"
        valid = base / "valid"
        valid.touch()
        bin_dir = base / "bin"
        bin_dir.mkdir()
        for tool in ("nix", "nix-store"):
            (bin_dir / tool).write_text(FAKE_NIX.format(
                python=sys.executable, fixture=str(self.fixture), calls=str(self.calls), valid=str(valid)))
            (bin_dir / tool).chmod(0o755)
        self.env = {k: v for k, v in os.environ.items() if k not in ("SEKKA_CI_ROOTS", "GITHUB_SHA")}
        self.env.update(PATH=f"{bin_dir}:{os.environ['PATH']}", GITHUB_STEP_SUMMARY=str(self.summary))

    def tearDown(self):
        self.tmp.cleanup()

    def check(self, name):
        return {"drv": f"/nix/store/{HASH}-vm-test-run-{name}.drv", "out": str(self.outputs / name)}

    def run_checks(self, names, failing=(), eval_fails=False):
        self.fixture.write_text(json.dumps({
            "checks": {name: self.check(name) for name in names},
            "failing": [f"vm-test-run-{name}" for name in failing],
            "eval_fails": eval_fails,
        }))
        return subprocess.run(
            [sys.executable, str(SCRIPTS / "ci-named-checks.py"), "--title", "Fixture checks",
             "--label", "aizome", ".#legacyPackages.x86_64-linux.fixture"],
            env=self.env, capture_output=True, text=True)

    def builds(self):
        return [json.loads(line) for line in self.calls.read_text().splitlines()]

    def test_every_check_passing_is_one_build_and_a_table(self):
        result = self.run_checks(["alpha", "beta"])
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn("Fixture checks: all 2 passed", result.stdout)
        self.assertNotIn("::error", result.stdout)
        [build] = self.builds()
        self.assertEqual(build[:3], ["build", "--keep-going", "--print-build-logs"])
        self.assertEqual(build[3:5], [self.check("alpha")["drv"] + "^out", self.check("beta")["drv"] + "^out"])
        summary = self.summary.read_text()
        self.assertIn("## Fixture checks: all 2 passed", summary)
        self.assertIn("| `alpha` | passed |", summary)
        self.assertIn("| `beta` | passed |", summary)

    def test_a_failure_is_named_with_its_own_log_and_the_rest_are_linked(self):
        result = self.run_checks(["alpha", "beta", "gamma"], failing=["beta"])
        self.assertEqual(result.returncode, 1)
        # What the failed build left at its output path does not make it pass.
        self.assertTrue((self.outputs / "beta").is_dir())
        self.assertIn("Fixture checks: 1 of 3 failed", result.stdout)
        # A colon ends a workflow command's properties, so the title's is escaped.
        self.assertIn("::error title=Fixture checks%3A beta::beta failed.", result.stdout)
        self.assertIn(f"nix log {self.check('beta')['drv']}", result.stdout)
        self.assertNotIn("title=Fixture checks%3A alpha", result.stdout)
        group = result.stdout.split("::group::beta: its log\n")[1].split("::endgroup::")[0].splitlines()
        # Workflow commands are off for the log's own lines.
        resume = group[0].removeprefix("::stop-commands::")
        self.assertRegex(resume, r"^[0-9a-f]{32}$")
        self.assertEqual(group[1:],
                         ["starting vm-test-run-beta", "\x1b[31massertion failed\x1b[0m <in> `vm-test-run-beta`",
                          f"::{resume}::"])
        # The whole build's output still passes through, before the report.
        self.assertIn("vm-test-run-alpha> finished", result.stdout.split("Fixture checks:")[0])
        # A failed build links nothing: a second one links the checks that passed.
        first, second = self.builds()
        self.assertEqual(len([a for a in first if a.endswith("^out")]), 3)
        self.assertEqual([a for a in second if a.endswith("^out")],
                         [self.check("alpha")["drv"] + "^out", self.check("gamma")["drv"] + "^out"])
        summary = self.summary.read_text()
        self.assertLess(summary.index("| `beta` | failed |"), summary.index("| `alpha` | passed |"))
        self.assertIn("<pre>starting vm-test-run-beta\nassertion failed &lt;in&gt; `vm-test-run-beta`</pre>", summary)
        self.assertIn(f"<code>{self.outputs / 'beta'}</code> keeps what the run wrote", summary)
        self.assertNotIn("\x1b", summary)

    def test_a_check_that_passed_before_is_not_run_again_and_says_so(self):
        self.assertEqual(self.run_checks(["alpha", "beta"], failing=["beta"]).returncode, 1)
        result = self.run_checks(["alpha", "beta"])
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn("alpha: passed before, not run again", result.stdout)
        self.assertRegex(result.stdout, r"beta: passed \(\d+ s\)")
        self.assertIn("| `alpha` | passed before, not run again |", self.summary.read_text())

    def test_a_build_command_that_fails_with_every_output_valid_still_fails(self):
        self.assertEqual(self.run_checks(["alpha"]).returncode, 0)
        # The wrapper refuses a roots directory that does not exist.
        self.env["SEKKA_CI_ROOTS"] = str(self.outputs / "missing")
        result = self.run_checks(["alpha"])
        self.assertEqual(result.returncode, 1)
        self.assertIn("alpha: passed before, not run again", result.stdout)
        self.assertIn("::error title=Fixture checks::Every check passed, but the build command ended with status 2",
                      result.stdout)

    def test_every_check_failing_links_nothing(self):
        result = self.run_checks(["alpha"], failing=["alpha"])
        self.assertEqual(result.returncode, 1)
        self.assertIn("Fixture checks: 1 of 1 failed", result.stdout)
        self.assertEqual(len(self.builds()), 1)

    def test_a_set_that_does_not_evaluate_or_is_empty_fails_before_building(self):
        for arguments in ({"eval_fails": True}, {}):
            result = self.run_checks([], **arguments)
            self.assertEqual(result.returncode, 1)
            self.assertIn("::error title=Fixture checks::", result.stdout)
            self.assertFalse(self.calls.exists())


if __name__ == "__main__":
    unittest.main()
