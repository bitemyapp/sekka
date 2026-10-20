"""Run the real publisher against an isolated filesystem standing in for S3."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
PUBLISHER = ROOT / "scripts/publish-nightly.sh"

AWS = r'''import json, os, pathlib, shutil, sys
args = sys.argv[1:]
root = pathlib.Path(os.environ["FAKE_S3_ROOT"])
with (root / "operations").open("a") as log:
    log.write(json.dumps(args) + "\n")
def target(url):
    assert url.startswith("s3://sekka-downloads/")
    return root / url.removeprefix("s3://sekka-downloads/")
if "s3api" in args:
    error = os.environ.get("FAIL_GET")
    source = root / args[args.index("--key") + 1]
    if error or not source.exists():
        sys.exit(error or "NoSuchKey")
    shutil.copyfile(source, args[-1])
elif "cp" in args:
    source, dest = args[-2:]
    if os.environ.get("FAIL_UPLOAD", "no-failure") in dest:
        sys.exit("Simulated upload failure")
    dest = target(dest)
    dest.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(source, dest)
elif "ls" in args:
    for path in sorted(target(args[-1]).rglob("*")):
        if path.is_file():
            print("2026-10-20 00:00:00", path.stat().st_size, path.relative_to(root))
elif "rm" in args:
    target(args[-1]).unlink()
else:
    sys.exit("Unexpected AWS command")
'''


class NightlyPublisher(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.bucket = self.root / "bucket"
        self.bucket.mkdir()
        self.bin = self.root / "bin"
        self.bin.mkdir()
        (self.bin / "aws").write_text(f"#!{sys.executable}\n" + AWS)
        (self.bin / "aws").chmod(0o755)
        self.env = dict(os.environ, FAKE_S3_ROOT=str(self.bucket),
                        PATH=str(self.bin) + os.pathsep + os.environ["PATH"])
        self.repo = self.root / "checkout"
        self.repo.mkdir()
        self.git("init", "--quiet")
        self.git("config", "user.name", "Nightly test")
        self.git("config", "user.email", "test@example.invalid")
        (self.repo / "flake.lock").write_text('{"test": "unchanged"}\n')
        self.git("add", "flake.lock")
        self.git("commit", "--quiet", "-m", "Fixture")
        self.sources = []
        for system in ("x86_64-linux", "aarch64-linux"):
            source = self.root / system
            source.mkdir()
            (source / f"test-{system}.iso").write_bytes(f"iso bytes for {system}".encode())
            self.sources.append(source)

    def git(self, *args):
        return subprocess.check_output(["git", "-C", str(self.repo), *args], text=True).strip()

    def publish(self, *sources, **env):
        return subprocess.run(["bash", str(PUBLISHER), *map(str, sources or self.sources)],
                              cwd=self.repo, env=dict(self.env, **env), capture_output=True, text=True)

    def seed(self, count=7):
        builds = []
        for i in range(count):
            build = {"id": f"old{i}", "revision": str(i) * 40, "iso": f"builds/old{i}/old.iso"}
            builds.append(build)
            folder = self.bucket / "nightly/builds" / build["id"]
            folder.mkdir(parents=True)
            (folder / "old.iso").write_bytes(b"old ISO")
            # Retention must preserve nested ARM files of kept revisions too.
            (folder / "aarch64-linux").mkdir()
            (folder / "aarch64-linux/old.iso").write_bytes(b"old ARM ISO")
        index = self.bucket / "nightly/builds.json"
        index.write_text(json.dumps({"builds": builds}))
        return index.read_bytes()

    def uploads(self):
        log = self.bucket / "operations"
        return [args for args in map(json.loads, log.read_text().splitlines()) if "cp" in args] if log.exists() else []

    def test_pair_is_checked_published_and_retained_as_one_build(self):
        self.seed()
        result = self.publish()
        self.assertEqual(result.returncode, 0, result.stderr)
        builds = json.loads((self.bucket / "nightly/builds.json").read_text())["builds"]
        self.assertEqual(len(builds), 7)
        new = builds[0]
        self.assertEqual(new["revision"], self.git("rev-parse", "HEAD"))
        self.assertEqual(new["system"], "x86_64-linux")
        self.assertEqual(new["iso"], new["images"][0]["iso"])
        self.assertEqual(new["sha256"], new["images"][0]["sha256"])
        self.assertEqual(new["bytes"], new["images"][0]["bytes"])
        self.assertEqual([image["system"] for image in new["images"]], ["x86_64-linux", "aarch64-linux"])
        for image, source in zip(new["images"], self.sources):
            data = next(source.glob("*.iso")).read_bytes()
            downloaded = self.bucket / "nightly" / image["iso"]
            self.assertEqual(downloaded.read_bytes(), data)
            self.assertEqual(image["bytes"], len(data))
            self.assertEqual(image["sha256"], hashlib.sha256(data).hexdigest())
            sums = (self.bucket / "nightly" / image["sha256sums"]).read_text()
            self.assertEqual(sums, f'{image["sha256"]}  {downloaded.name}\n')
        self.assertEqual((self.bucket / "nightly" / new["flake_lock"]).read_bytes(),
                         (self.repo / "flake.lock").read_bytes())
        self.assertEqual(json.loads((self.bucket / "nightly" / new["manifest"]).read_text()), new)
        self.assertTrue((self.bucket / "nightly/builds/old5/aarch64-linux/old.iso").exists())
        self.assertFalse((self.bucket / "nightly/builds/old6").joinpath("old.iso").exists())
        uploads = self.uploads()
        self.assertTrue(uploads[-1][-1].endswith("nightly/builds.json"))
        self.assertEqual(sum(arg[-1].endswith(".iso") for arg in uploads), 2)

    def test_first_publication_and_repeated_revision_do_not_duplicate_it(self):
        for _ in range(2):
            result = self.publish()
            self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(len(json.loads((self.bucket / "nightly/builds.json").read_text())["builds"]), 1)

    def test_failed_upload_keeps_index_and_all_previous_builds(self):
        before = self.seed()
        for failure in ("/aarch64-linux/", "/manifest.json", "/nightly/builds.json"):
            with self.subTest(failure=failure):
                result = self.publish(FAIL_UPLOAD=failure)
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual((self.bucket / "nightly/builds.json").read_bytes(), before)
                self.assertTrue((self.bucket / "nightly/builds/old6/old.iso").exists())

    def test_rejects_swapped_missing_empty_multiple_and_dirty_inputs_before_upload(self):
        self.assertNotEqual(self.publish(*reversed(self.sources)).returncode, 0)
        arm = next(self.sources[1].glob("*.iso"))
        original = arm.read_bytes()
        arm.unlink()
        self.assertNotEqual(self.publish().returncode, 0)
        arm.touch()
        self.assertNotEqual(self.publish().returncode, 0)
        arm.write_bytes(original)
        extra = self.sources[1] / "extra-aarch64-linux.iso"
        extra.write_bytes(original)
        self.assertNotEqual(self.publish().returncode, 0)
        extra.unlink()
        (self.repo / "flake.lock").write_text("changed")
        self.assertNotEqual(self.publish().returncode, 0)
        self.assertEqual(self.uploads(), [])

    def test_does_not_treat_access_or_invalid_index_as_no_builds(self):
        before = self.seed()
        self.assertNotEqual(self.publish(FAIL_GET="AccessDenied").returncode, 0)
        self.assertEqual((self.bucket / "nightly/builds.json").read_bytes(), before)
        (self.bucket / "nightly/builds.json").write_text("not JSON")
        self.assertNotEqual(self.publish().returncode, 0)
        self.assertEqual(self.uploads(), [])


if __name__ == "__main__":
    unittest.main()
