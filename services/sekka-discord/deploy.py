#!/usr/bin/env python3
"""Deploy Yuki's package from the latest successful Yuki workflow run on main; one bounded tick."""

import argparse
import fcntl
import functools
import http.client
import json
import os
from pathlib import Path
import re
import secrets
import signal
import ssl
import stat
import subprocess
import sys
import time
import urllib.parse

BOT = "sekka-discord.service"
MONITOR = "sekka-discord-monitor.service"
MONITOR_TIMER = "sekka-discord-monitor.timer"
BINARIES = ("sekka-discord", "sekka-discord-monitor")
NIX = ["nix", "--extra-experimental-features", "nix-command flakes"]
MAX_HTTP = 256 * 1024
MAX_STATE = 65536
MAX_FAILED = 64
RETRY = 900
BUILD_ATTEMPTS = 3
KEEP = 5
SHA = re.compile(r"[0-9a-f]{40}")
KEY = re.compile(r"[0-9]{1,20}\.[0-9]{1,6}")
STORE_PATH = re.compile(r"/nix/store/[0-9a-df-np-sv-z]{32}-[A-Za-z0-9+._?=-]+")


class DeployError(Exception):
    """Only fixed descriptions, commits and store paths may leave the agent."""


class BuildError(DeployError):
    pass


class RateLimited(DeployError):
    def __init__(self, until):
        super().__init__("GitHub API rate limit; retry scheduled")
        self.until = until


class TooLarge(DeployError):
    pass


def log(message):
    print(message, flush=True)


def bounded_http(function):
    """A slow peer cannot stretch per-socket timeouts by trickling bytes."""
    @functools.wraps(function)
    def call(*args, **kwargs):
        def expired(_number, _frame):
            raise TimeoutError("HTTP wall-clock deadline exceeded")

        previous = signal.signal(signal.SIGALRM, expired)
        signal.setitimer(signal.ITIMER_REAL, 8)
        try:
            return function(*args, **kwargs)
        finally:
            signal.setitimer(signal.ITIMER_REAL, 0)
            signal.signal(signal.SIGALRM, previous)
    return call


def finite_number(value):
    return type(value) in (int, float) and 0 <= value <= 10**15


def positive_int(value):
    return type(value) is int and 0 < value < 10**20


def rate_limit_until(response, now):
    for header, base in (("retry-after", now), ("x-ratelimit-reset", 0)):
        value = response.getheader(header)
        if value and value.isdigit():
            return min(now + 3600, max(now + 60, base + int(value)))
    return now + RETRY


class GitHub:
    def __init__(self, api, repository):
        url = urllib.parse.urlsplit(api)
        if url.scheme != "https" or not url.hostname or url.path not in ("", "/") or url.query:
            raise DeployError("SEKKA_DEPLOY_API must be an https origin")
        self.host = url.hostname
        self.port = url.port or 443
        self.repository = repository

    @bounded_http
    def get(self, path, etag=None):
        """(None, etag) when unchanged since etag, otherwise (body, new etag)."""
        connection = http.client.HTTPSConnection(self.host, self.port, timeout=8,
                                                 context=ssl.create_default_context())
        headers = {"Accept": "application/vnd.github+json", "X-GitHub-Api-Version": "2022-11-28",
                   "User-Agent": "SekkaYukiDeploy/1.0"}
        if etag:
            headers["If-None-Match"] = etag
        try:
            connection.request("GET", path, headers=headers)
            response = connection.getresponse()
            raw = response.read(MAX_HTTP + 1)
            if response.status == 304 and etag:
                return None, etag
            if response.status in (403, 429) and (response.getheader("x-ratelimit-remaining") == "0"
                                                  or response.getheader("retry-after")):
                raise RateLimited(rate_limit_until(response, time.time()))
            if response.status != 200:
                raise DeployError("GitHub API rejected request")
            if len(raw) > MAX_HTTP:
                raise TooLarge("GitHub API response exceeds its size limit")
            try:
                return json.loads(raw), response.getheader("etag")
            except (ValueError, UnicodeError):
                raise DeployError("GitHub API response was invalid") from None
        except (OSError, http.client.HTTPException):
            raise DeployError("GitHub API unavailable") from None
        finally:
            connection.close()

    def latest_run(self, state, workflow, branch):
        query = urllib.parse.urlencode({"branch": branch, "event": "workflow_dispatch", "status": "success",
                                        "per_page": 1, "exclude_pull_requests": "true"})
        body, etag = self.get(f"/repos/{self.repository}/actions/workflows/{workflow}/runs?{query}", state["etag"])
        if body is None:
            return state["run"]
        runs = body.get("workflow_runs") if isinstance(body, dict) else None
        if not isinstance(runs, list):
            raise DeployError("GitHub workflow runs response was invalid")
        state["run"] = parse_run(runs[0], self.repository, workflow, branch) if runs else None
        state["etag"] = etag if isinstance(etag, str) and len(etag) <= 256 else None
        return state["run"]

    def jobs_succeeded(self, run):
        body, _ = self.get(f"/repos/{self.repository}/actions/runs/{run['id']}/attempts/{run['attempt']}/jobs?per_page=100")
        jobs = body.get("jobs") if isinstance(body, dict) else None
        if not isinstance(jobs, list) or body.get("total_count") != len(jobs):
            raise DeployError("GitHub jobs response was invalid")
        # A job skipped by its main-only guard still leaves the run successful.
        return bool(jobs) and all(isinstance(job, dict) and job.get("status") == "completed"
                                  and job.get("conclusion") == "success" and job.get("head_sha") == run["sha"]
                                  for job in jobs)

    def branch_head(self, branch):
        """The commit refs/heads/<branch> names; a tag of the same name is a different reference."""
        body, _ = self.get(f"/repos/{self.repository}/git/ref/heads/{branch}")
        try:
            valid = (body["ref"] == "refs/heads/" + branch and body["object"]["type"] == "commit"
                     and SHA.fullmatch(body["object"]["sha"]) is not None)
        except (KeyError, TypeError):
            valid = False
        if not valid:
            raise DeployError("GitHub branch response was invalid")
        return body["object"]["sha"]

    def on_branch(self, sha, branch):
        """Whether the commit is the branch's head or one of its ancestors.

        A run's own branch name does not say so: GitHub reports a tag's name in
        the same field, and a run dispatched on a tag takes its workflow from
        the tag's commit.
        """
        head = self.branch_head(branch)
        if sha == head:
            return True
        # Seen from the head, an ancestor is only behind: no commits and no files
        # to list, so the answer is small. Anything else may be any size.
        try:
            body, _ = self.get(f"/repos/{self.repository}/compare/{head}...{sha}")
        except TooLarge:
            return False
        try:
            return body["status"] == "behind" and body["ahead_by"] == 0 and body["merge_base_commit"]["sha"] == sha
        except (KeyError, TypeError):
            raise DeployError("GitHub comparison response was invalid") from None


def parse_run(run, repository, workflow, branch):
    try:
        valid = (positive_int(run["id"]) and positive_int(run["run_attempt"])
                 and SHA.fullmatch(run["head_sha"]) is not None and run["head_branch"] == branch
                 and run["event"] == "workflow_dispatch" and run["status"] == "completed"
                 and run["conclusion"] == "success" and run["path"] == ".github/workflows/" + workflow
                 and run["repository"]["full_name"].lower() == repository.lower()
                 and run["head_repository"]["full_name"].lower() == repository.lower())
    except (KeyError, TypeError, AttributeError):
        valid = False
    if not valid:
        raise DeployError("Latest workflow run does not match the configured repository, branch and workflow")
    return {"id": run["id"], "attempt": run["run_attempt"], "sha": run["head_sha"]}


class Store:
    def __init__(self, directory):
        self.path = Path(directory)
        self.lock = None

    def __enter__(self):
        self.path.mkdir(mode=0o755, parents=True, exist_ok=True)
        info = self.path.lstat()
        # Yuki's identities traverse this directory to reach the deployed profile.
        if not stat.S_ISDIR(info.st_mode) or info.st_uid != os.getuid() or info.st_mode & 0o022:
            raise DeployError("Deploy state directory must be owned by this user and writable only by it")
        fd = os.open(self.path / "deploy.lock", os.O_CREAT | os.O_RDWR | os.O_NOFOLLOW, 0o600)
        self.lock = os.fdopen(fd, "a+")
        try:
            fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            self.lock.close()
            self.lock = None
            raise DeployError("Another deploy invocation holds the process lock") from None
        return self

    def __exit__(self, *_):
        if self.lock:
            self.lock.close()

    def load(self):
        try:
            fd = os.open(self.path / "state.json", os.O_RDONLY | os.O_NOFOLLOW)
        except FileNotFoundError:
            return new_state()
        with os.fdopen(fd, "rb") as source:
            info = os.fstat(source.fileno())
            if not stat.S_ISREG(info.st_mode) or info.st_uid != os.getuid() or info.st_mode & 0o077:
                raise DeployError("Deploy state file must be private and owned by this user")
            raw = source.read(MAX_STATE + 1)
        if len(raw) > MAX_STATE:
            raise DeployError("Deploy state exceeds its size limit")
        try:
            state = json.loads(raw)
            if isinstance(state, dict):
                # State written before runs were ordered: the next tick records a floor.
                state.setdefault("floor", None)
            validate_state(state)
        except (ValueError, TypeError, KeyError):
            raise DeployError("Deploy state is invalid; refusing to forget deployment history") from None
        return state

    def save(self, state):
        encoded = json.dumps(state, separators=(",", ":"), allow_nan=False).encode()
        if len(encoded) > MAX_STATE:
            raise DeployError("Deploy state exceeds its size limit")
        temporary = self.path / (".state-" + secrets.token_hex(8))
        try:
            fd = os.open(temporary, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
            with os.fdopen(fd, "wb") as target:
                target.write(encoded)
                target.flush()
                os.fsync(target.fileno())
            os.replace(temporary, self.path / "state.json")
            directory = os.open(self.path, os.O_RDONLY | os.O_DIRECTORY)
            try:
                os.fsync(directory)
            finally:
                os.close(directory)
        finally:
            temporary.unlink(missing_ok=True)


def new_state():
    # floor is the highest run this host has settled: the latest one when it
    # first looked, which it never deploys, and from then on each run it deploys.
    return {"version": 1, "etag": None, "run": None, "deployed": None, "floor": None, "failed": [], "retry": None,
            "switching": None, "next_attempt": 0}


def validate_state(state):
    if not isinstance(state, dict) or state.get("version") != 1:
        raise ValueError()
    if state["etag"] is not None and (not isinstance(state["etag"], str) or len(state["etag"]) > 256):
        raise ValueError()
    run = state["run"]
    if run is not None and not (positive_int(run["id"]) and positive_int(run["attempt"]) and SHA.fullmatch(run["sha"])):
        raise ValueError()
    if state["deployed"] is not None and not SHA.fullmatch(state["deployed"]):
        raise ValueError()
    floor = state["floor"]
    if floor is not None and not (type(floor) is int and 0 <= floor < 10**20):
        raise ValueError()
    failed = state["failed"]
    if not isinstance(failed, list) or len(failed) > MAX_FAILED or not all(isinstance(k, str) and KEY.fullmatch(k) for k in failed):
        raise ValueError()
    retry = state["retry"]
    if retry is not None and not (KEY.fullmatch(retry["key"]) and type(retry["count"]) is int
                                  and 0 < retry["count"] < BUILD_ATTEMPTS):
        raise ValueError()
    switching = state["switching"]
    if switching is not None and not (KEY.fullmatch(switching["key"]) and SHA.fullmatch(switching["rev"])
                                      and (switching["previous"] is None
                                           or STORE_PATH.fullmatch(switching["previous"]))):
        raise ValueError()
    if not finite_number(state["next_attempt"]):
        raise ValueError()


class System:
    """Local effects: Nix builds, the deployment profile, and Yuki's units."""

    clock = staticmethod(time.monotonic)
    sleep = staticmethod(time.sleep)

    def __init__(self, directory, fallback, installable):
        self.directory = Path(directory)
        self.profile = self.directory / "profile"
        self.candidate = self.directory / "candidate"
        self.fallback = fallback
        self.installable = installable

    def run(self, args, timeout):
        # Nix's own diagnostics go to the journal through inherited stderr.
        try:
            result = subprocess.run(args, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, timeout=timeout)
        except (OSError, subprocess.SubprocessError):
            raise DeployError("Local command failed to complete") from None
        return result.returncode, result.stdout[:65536]

    def build(self, rev):
        code, out = self.run([*NIX, "build", "--no-write-lock-file", "--option", "accept-flake-config", "false",
                              "--out-link", str(self.candidate), "--print-out-paths",
                              self.installable.replace("{rev}", rev)], 1800)
        if code:
            raise BuildError("Build failed")
        path = out.decode("ascii", "replace").strip()
        if not STORE_PATH.fullmatch(path) or os.path.realpath(self.candidate) != path:
            raise DeployError("Build did not produce exactly one store path")
        if not all(os.access(Path(path, "bin", name), os.X_OK) for name in BINARIES):
            raise DeployError("Built package lacks Yuki's programs")
        return path

    def deployed(self):
        """The package the profile names, or None while Yuki runs this host's own."""
        path = os.path.realpath(self.profile) if self.profile.is_symlink() else ""
        return path if STORE_PATH.fullmatch(path) else None

    def current(self):
        return self.deployed() or self.fallback

    def generations(self):
        return sorted((int(match[1]), entry) for entry in self.directory.iterdir()
                      if (match := re.fullmatch(r"profile-([0-9]{1,9})-link", entry.name)))

    def switch(self, path):
        code, _ = self.run([*NIX, "build", "--no-link", "--profile", str(self.profile), path], 300)
        if code or os.path.realpath(self.profile) != path:
            raise DeployError("Deployment profile could not be switched")
        # The generation just made is the newest, so the oldest links are never current.
        for _, entry in self.generations()[:-KEEP]:
            entry.unlink()

    def restore(self, previous):
        """Return to an earlier deployed package, or with None to this host's own.

        Yuki's launchers prefer any profile to the host's package, so a profile
        that merely named the host's package would hide every later update of it.
        """
        if previous is not None:
            self.switch(previous)
            return
        self.profile.unlink(missing_ok=True)
        for _, entry in self.generations():
            entry.unlink()

    def discard_candidate(self):
        if self.candidate.is_symlink():
            self.candidate.unlink()

    def unit(self, name):
        code, out = self.run(["systemctl", "show", name, "--no-pager",
                              "--property=InvocationID,ActiveState,Result,ExecMainStatus"], 10)
        if code:
            raise DeployError("systemd unit status unavailable")
        return dict(line.split("=", 1) for line in out.decode("ascii", "replace").splitlines() if "=" in line)

    def request_restart(self):
        # sekka-discord-deploy-restart.path restarts the bot as root; its content is never read.
        fd = os.open(self.directory / "restart", os.O_WRONLY | os.O_CREAT | os.O_TRUNC | os.O_NOFOLLOW, 0o600)
        with os.fdopen(fd, "w") as target:
            target.write(secrets.token_hex(8) + "\n")

    @bounded_http
    def healthy(self):
        connection = http.client.HTTPConnection("127.0.0.1", 8787, timeout=3)
        try:
            connection.request("GET", "/healthz")
            response = connection.getresponse()
            return response.status == 200 and len(response.read(4097)) <= 4096
        except (OSError, http.client.HTTPException):
            return False
        finally:
            connection.close()


def restart(system):
    """The bot must start a new invocation that answers health checks and stays up.

    systemd gives every start a new invocation ID, automatic restarts included.
    """
    before = system.unit(BOT).get("InvocationID", "")
    system.request_restart()
    deadline = system.clock() + 90
    unit = {}
    while unit.get("InvocationID", before) == before or unit.get("ActiveState") != "active":
        if system.clock() > deadline:
            return False
        system.sleep(1)
        unit = system.unit(BOT)
    deadline = system.clock() + 30
    while not system.healthy():
        if system.clock() > deadline:
            return False
        system.sleep(1)
    system.sleep(10)
    after = system.unit(BOT)
    return after.get("InvocationID") == unit["InvocationID"] and after.get("ActiveState") == "active" and system.healthy()


def monitor_regressed(system, before, baseline):
    """A monitor whose last tick succeeded must succeed again on its first tick with the new package."""
    if before.get("Result") != "success" or before.get("ExecMainStatus") != "0":
        return False
    if system.unit(MONITOR_TIMER).get("ActiveState") != "active":
        return False
    deadline = system.clock() + 150
    while system.clock() <= deadline:
        unit = system.unit(MONITOR)
        if unit.get("InvocationID", baseline) != baseline and unit.get("ActiveState") in ("inactive", "failed"):
            return unit.get("Result") != "success" or unit.get("ExecMainStatus") != "0"
        system.sleep(2)
    return True


def deploy(system, path, previous):
    """previous is the deployed package to return to, or None for this host's own."""
    monitor = system.unit(MONITOR)
    system.switch(path)
    # A tick already running when the profile switched may still be the old package.
    baseline = system.unit(MONITOR).get("InvocationID", "")
    if restart(system) and not monitor_regressed(system, monitor, baseline):
        return True
    system.restore(previous)
    if not restart(system):
        log("DEPLOY FAILED the restored package did not pass health checks either")
    return False


def fail(state, key):
    state["failed"] = (state["failed"] + [key])[-MAX_FAILED:]


def named(previous):
    return previous or "this host's package"


def recover(state, system):
    switching = state["switching"]
    previous = switching["previous"]
    log(f"DEPLOY FAILED rev={switching['rev']}: an interrupted deployment was rolled back to {named(previous)}")
    if system.deployed() != previous:
        system.restore(previous)
    restart(system)
    fail(state, switching["key"])
    state["switching"] = None


def tick(state, github, system, workflow, branch, now, save):
    if state["switching"]:
        recover(state, system)
        save()
    if now < state["next_attempt"]:
        return "waiting to retry"
    try:
        run = github.latest_run(state, workflow, branch)
        if state["floor"] is None:
            # A run asks the hosts that are listening to deploy its commit. One
            # from before this host listened was not asked of it, and its commit
            # may be far older than the package the host already runs.
            state["floor"] = run["id"] if run else 0
            log(f"DEPLOY baseline run={state['floor']}: only later runs are deployed")
            return "baseline recorded"
        if run is None:
            return "no successful run"
        key = f"{run['id']}.{run['attempt']}"
        if run["sha"] == state["deployed"] or key in state["failed"] or run["id"] == state["floor"]:
            return "up to date"
        if run["id"] < state["floor"]:
            # Run numbers only grow, so the latest run is an older one only once
            # later runs have been deleted: never a reason to go back to its commit.
            fail(state, key)
            log(f"DEPLOY FAILED run={run['id']}: older than run {state['floor']}, which this host already took; "
                "a later run is gone")
            return "refused"
        succeeded = github.jobs_succeeded(run)
        on_branch = succeeded and github.on_branch(run["sha"], branch)
    except RateLimited as error:
        state["next_attempt"] = error.until
        return "waiting for the GitHub API rate limit"
    if not succeeded:
        fail(state, key)
        log(f"DEPLOY skipped run={run['id']}: not every job succeeded")
        return "skipped"
    rev = run["sha"]
    if not on_branch:
        fail(state, key)
        log(f"DEPLOY FAILED rev={rev} run={run['id']}: the commit is not on {branch}")
        return "refused"
    log(f"DEPLOY started rev={rev} run={run['id']} attempt={run['attempt']}")
    try:
        try:
            path = system.build(rev)
        except BuildError:
            retry = state["retry"] if state["retry"] and state["retry"]["key"] == key else {"key": key, "count": 0}
            retry["count"] += 1
            if retry["count"] >= BUILD_ATTEMPTS:
                state["retry"] = None
                fail(state, key)
                log(f"DEPLOY FAILED rev={rev}: the build failed {BUILD_ATTEMPTS} times")
            else:
                state["retry"] = retry
                state["next_attempt"] = now + RETRY
                log(f"DEPLOY build failed rev={rev}; attempt {retry['count']} of {BUILD_ATTEMPTS}, retrying in {RETRY}s")
            raise
        except DeployError as error:
            fail(state, key)
            log(f"DEPLOY FAILED rev={rev}: {error}")
            raise
        state["retry"] = None
        if path == system.current():
            state["deployed"], state["floor"] = rev, run["id"]
            log(f"DEPLOY unchanged rev={rev} path={path}")
            return "unchanged"
        previous = system.deployed()
        # A crash from here on is rolled back by the next tick rather than trusted.
        state["switching"] = {"key": key, "rev": rev, "previous": previous}
        save()
        healthy = deploy(system, path, previous)
        state["switching"] = None
        if not healthy:
            fail(state, key)
            log(f"DEPLOY FAILED rev={rev}: health checks did not pass; restored {named(previous)}")
            raise DeployError("Deployment was rolled back")
        state["deployed"], state["floor"] = rev, run["id"]
        log(f"DEPLOY succeeded rev={rev} path={path}")
        return "deployed"
    finally:
        system.discard_candidate()


def setting(name, pattern, default=None):
    value = os.environ.get(name, default)
    if value is None or not re.fullmatch(pattern, value):
        raise DeployError(f"Set {name} to a valid value")
    return value


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--once", action="store_true", help="check for one new successful run and deploy it (default)")
    parser.parse_args(argv)
    os.umask(0o077)
    try:
        repository = setting("SEKKA_DEPLOY_REPOSITORY", r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+")
        workflow = setting("SEKKA_DEPLOY_WORKFLOW", r"[A-Za-z0-9_.-]+\.ya?ml", "yuki.yml")
        branch = setting("SEKKA_DEPLOY_BRANCH", r"[A-Za-z0-9_.-]+(/[A-Za-z0-9_.-]+)*", "main")
        installable = setting("SEKKA_DEPLOY_INSTALLABLE", r"[^\s]*\{rev\}[^\s]*")
        fallback = setting("SEKKA_DEPLOY_FALLBACK", STORE_PATH.pattern)
        directory = setting("SEKKA_DEPLOY_STATE_DIR", r"/[^\s]+")
        github = GitHub(os.environ.get("SEKKA_DEPLOY_API", "https://api.github.com"), repository)
        with Store(directory) as store:
            state = store.load()
            try:
                result = tick(state, github, System(directory, fallback, installable), workflow, branch,
                              time.time(), lambda: store.save(state))
            finally:
                store.save(state)
            print("sekka-discord-deploy: " + result)
            return 0
    except DeployError as error:
        print("sekka-discord-deploy: " + str(error), file=sys.stderr)
    except Exception:
        print("sekka-discord-deploy: deploy failed; private state retained, check the journal and unit status", file=sys.stderr)
    return 1


if __name__ == "__main__":
    sys.exit(main())
