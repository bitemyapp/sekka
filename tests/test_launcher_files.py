#!/usr/bin/env python3
"""Real Files provider/socket checks in private directories, never the host home.

Usage: test_launcher_files.py ELEPHANT FILES_CONFIG [--original]
The original control must miss a new file until its daemon restarts. The fixed
control exercises fsnotify and fd's inclusion policy, including directory moves.
"""
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import time


package = Path(sys.argv[1])
configuration = Path(sys.argv[2])
original = "--original" in sys.argv[3:]


class Provider:
    def __init__(self, root):
        self.root = root
        self.home = root / "home"
        for name in ("home", "cache", "run", "config", "providers", "bin"):
            (root / name).mkdir()
        (root / "run").chmod(0o700)
        (root / "providers/files.so").symlink_to(package / "lib/elephant/providers/files.so")
        if configuration.is_file():
            shutil.copy(configuration, root / "config/files.toml")
        self.env = dict(os.environ, HOME=str(self.home), XDG_CONFIG_HOME=str(root / "config"),
                        XDG_CONFIG_DIRS=str(root / "config"), XDG_CACHE_HOME=str(root / "cache"),
                        XDG_DATA_HOME=str(root / "data"), XDG_DATA_DIRS=str(root / "data"),
                        XDG_RUNTIME_DIR="run", ELEPHANT_PROVIDER_DIR=str(root / "providers"))
        # Fault injection surrounds the real fd executable, outside the indexed
        # home. Only a chosen recursive scan fails; ordinary inclusion queries
        # and the production provider/package remain unchanged.
        fd = shutil.which("fd")
        assert fd
        wrapper = root / "bin/fd"
        wrapper.write_text(f"#!{sys.executable}\n" + """
import os, pathlib, sys
root = pathlib.Path(__file__).parent.parent
fault = root / "fail-scan"
if fault.exists() and "--max-depth" not in sys.argv and sys.argv[2] == fault.read_text():
    with (root / "failed-scans").open("a") as log:
        log.write(repr(sys.argv) + "\\n")
    sys.exit(73)
""" + f"os.execv({fd!r}, [{fd!r}] + sys.argv[1:])\n")
        wrapper.chmod(0o755)
        self.env["PATH"] = str(root / "bin") + os.pathsep + os.environ["PATH"]
        self.binary = package / "bin/.elephant-wrapped"
        self.log = open(root / "provider.log", "w")
        self.daemon = None
        self.home.joinpath("sentinel.txt").write_text("original indexed file")
        self.home.joinpath("Documents").mkdir()
        self.home.joinpath(".ignore").write_text("ignored-tree/\nignored.txt\n")
        try:
            self.start()
        except BaseException:
            self.stop()
            self.log.close()
            print(root.joinpath("provider.log").read_text(), flush=True)
            raise

    def start(self):
        self.daemon = subprocess.Popen([self.binary, "--config", self.root / "config"],
                                       env=self.env, cwd=self.root, stdout=self.log, stderr=subprocess.STDOUT)
        self.expect("sentinel.txt", {self.home / "sentinel.txt"})

    def stop(self):
        if self.daemon is not None:
            self.daemon.terminate()
            self.daemon.wait(10)
            self.daemon = None

    def query(self, text):
        if not self.root.joinpath("run/elephant/elephant.sock").exists():
            return set()
        result = subprocess.run([self.binary, "query", "--json", f"files;{text};1000;false"],
                                env=self.env, cwd=self.root, capture_output=True, text=True, timeout=10)
        items = [json.loads(line).get("item") for line in result.stdout.splitlines() if line.startswith("{")]
        return {Path(item["text"]) for item in items if item}

    def expect(self, text, paths, seconds=15):
        deadline = time.monotonic() + seconds
        actual = set()
        while time.monotonic() < deadline:
            if self.daemon.poll() is not None:
                raise AssertionError("provider exited")
            actual = self.query(text)
            if actual == paths:
                print("FILES", text, sorted(str(p.relative_to(self.home)) for p in actual), flush=True)
                return
            time.sleep(0.15)
        raise AssertionError((text, sorted(map(str, actual)), sorted(map(str, paths))))


with tempfile.TemporaryDirectory(prefix="tatami-files-") as temporary:
    root = Path(temporary)
    provider = Provider(root)
    try:
        h = provider.home
        fresh = h / "Documents/new-note.txt"
        fresh.write_text("saved after provider start")
        if original:
            time.sleep(3)
            assert provider.query("new-note") == set(), "original unexpectedly indexed new file"
            provider.stop()
            provider.start()
            provider.expect("new-note", {fresh})
            # Editors commonly save a hidden temporary file and rename over
            # the old inode. The final path must survive coalesced remove/create.
            temporary_save = h / "Documents/.atomic-save"
            temporary_save.write_text("replacement document")
            temporary_save.replace(fresh)
            time.sleep(3)
            provider.expect("new-note", {fresh})
            assert fresh.read_text() == "replacement document"
            unusual = {h / "Documents/ spaced name ", h / "Documents/line\nbreak.txt"}
            for path in unusual:
                path.write_text("valid filename")
            provider.expect("spaced name", {h / "Documents/ spaced name "})
            provider.expect("break.txt", {h / "Documents/line\nbreak.txt"})
            print("ORIGINAL NEGATIVE: new save missing until restart", flush=True)
        else:
            provider.expect("new-note", {fresh})
            directory = h / "FreshRoot"
            (directory / "Nested").mkdir(parents=True)
            child = directory / "Nested/child-note.txt"
            child.write_text("created immediately with a new directory")
            provider.expect("FreshRoot", {directory, directory / "Nested", child})
            renamed = directory / "Nested/renamed-note.txt"
            child.rename(renamed)
            provider.expect("child-note", set())
            provider.expect("renamed-note", {renamed})
            moved = h / "MovedRoot"
            directory.rename(moved)
            provider.expect("FreshRoot", set())
            provider.expect("MovedRoot", {moved, moved / "Nested", moved / "Nested/renamed-note.txt"})
            later = moved / "Nested/later-note.txt"
            later.write_text("watch follows renamed directory")
            provider.expect("later-note", {later})
            root.joinpath("fail-scan").write_text(str(moved))
            moved.joinpath(".ignore").write_text("renamed-note.txt\n")
            deadline = time.monotonic() + 15
            while not root.joinpath("failed-scans").exists() and time.monotonic() < deadline:
                time.sleep(0.1)
            assert root.joinpath("failed-scans").exists(), "recursive scan fault was not exercised"
            assert provider.query("renamed-note") == {moved / "Nested/renamed-note.txt"}, "failed scan must preserve index"
            during_failure = moved / "Nested/during-failure.txt"
            during_failure.write_text("old descendant watcher remains active")
            provider.expect("during-failure", {during_failure})
            root.joinpath("fail-scan").unlink()
            provider.expect("renamed-note", set())
            print("SCAN RECOVERY", root.joinpath("failed-scans").read_text(), flush=True)
            moved.joinpath(".ignore").unlink()
            provider.expect("renamed-note", {moved / "Nested/renamed-note.txt"})
            sibling = h / "Documents/new-note.txt-other"
            sibling.write_text("must survive prefix deletion")
            provider.expect("new-note", {fresh, sibling})
            fresh.unlink()
            provider.expect("new-note", {sibling})
            wildcard = h / "Documents/a_%_.txt"
            literal_sibling = h / "Documents/a_more_.txt"
            wildcard.write_text("literal SQL wildcard name")
            literal_sibling.write_text("unrelated sibling")
            provider.expect("Documents/a_", {wildcard, literal_sibling})
            wildcard.unlink()
            provider.expect("Documents/a_", {literal_sibling})
            (h / ".hidden-note.txt").write_text("hidden")
            (h / ".hidden-dir").mkdir()
            (h / ".hidden-dir/private-note.txt").write_text("hidden child")
            (h / "ignored-tree").mkdir()
            (h / "ignored-tree/ignored-note.txt").write_text("ignored child")
            (h / "ignored.txt").write_text("ignored file")
            time.sleep(3)
            assert not provider.query("hidden") and not provider.query("private-note")
            assert not provider.query("ignored"), "watch updates bypass fd ignore policy"
            h.joinpath(".ignore").write_text("ignored-tree/\nignored.txt\nMovedRoot/\n")
            provider.expect("MovedRoot", set())
            later.write_text("must stay excluded after watcher removal")
            time.sleep(3)
            assert not provider.query("later-note")
            h.joinpath(".ignore").write_text("ignored-tree/\nignored.txt\n")
            provider.expect("MovedRoot", {moved, moved / "Nested", moved / "Nested/renamed-note.txt", later, during_failure})
            shutil.rmtree(moved)
            provider.expect("MovedRoot", set())
            print("FIXED: live create/atomic replace/rename/move/delete, root watches, scan recovery, filename boundaries and fd policy passed", flush=True)
    finally:
        provider.stop()
        provider.log.close()
        print(root.joinpath("provider.log").read_text(), flush=True)
