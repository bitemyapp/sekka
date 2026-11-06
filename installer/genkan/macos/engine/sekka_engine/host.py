# SPDX-License-Identifier: GPL-3.0-or-later
"""Everything the engine does to the Mac outside Python goes through a Host.

`MacHost` is the only implementation a real run uses. It runs a fixed set of
Apple's own tools, found only in the system directories, with a fixed
environment and nothing on standard input unless the caller supplies it.
Asahi's modules reach it through `SubprocessShim`, which replaces the
`subprocess` they imported, so every command Asahi runs is checked against
the same list. The test suite substitutes its own Host; nothing on the
command line or in the environment can.
"""

import errno
import fcntl
import logging
import os
import re
import stat
import subprocess
import sys

log = logging.getLogger("sekka_engine.host")

SYSTEM_PATH = ("/usr/bin", "/bin", "/usr/sbin", "/sbin")
ENVIRONMENT = {"PATH": ":".join(SYSTEM_PATH), "LC_ALL": "C", "LANG": "C"}

# The tools Asahi's modules and the adapter run. Nothing else is started.
COMMANDS = frozenset(
    {
        "bless",  # set and read the startup disk
        "bputil",  # read boot policies (Asahi's boot-mode and stub checks)
        "chflags",  # Asahi's admin-user copy
        "diskutil",
        "fdesetup",  # FileVault status
        "hdiutil",  # Asahi attaches the stub's recovery image for firmware
        "ioreg",
        "mount",  # Asahi's remount_rw
        "newfs_msdos",  # Asahi formats the ESP
        "nvram",
        "pmset",  # power source
        "scutil",  # console user, computer name
        "sync",
        "sysctl",  # Asahi's bug checks
        "log",  # Asahi's bug checks
        "tar",  # Asahi's firmware archive
        "xattr",  # Asahi's stub icon
    }
)

_RAW_DEVICE = re.compile(r"disk[0-9]+s[0-9]+\Z")

# fcntl and ioctl numbers from <sys/fcntl.h> and <sys/disk.h>.
F_FULLFSYNC = 51
# The most read_small_file reads: preferences and account records are far
# smaller.
SMALL_FILE_LIMIT = 1024 * 1024
DKIOCSYNCHRONIZECACHE = 0x20006416  # _IO('d', 22)


class CommandRefused(RuntimeError):
    """A command outside the engine's list, or named by a path."""


class Completed:
    def __init__(self, argv, returncode, stdout, stderr):
        self.argv = list(argv)
        self.returncode = returncode
        self.stdout = stdout
        self.stderr = stderr

    @property
    def output(self):
        return (self.stdout or b"") + (self.stderr or b"")


class Host:
    """The interface. A real run uses MacHost; tests substitute their own."""

    def run(self, argv, input=None, timeout=None):  # pragma: no cover - interface
        raise NotImplementedError

    def open_raw(self, device, writable):  # pragma: no cover - interface
        raise NotImplementedError

    def flush_device(self, fileobj):  # pragma: no cover - interface
        raise NotImplementedError

    def read_small_file(self, path, owners, limit=SMALL_FILE_LIMIT):  # pragma: no cover - interface
        raise NotImplementedError

    def user(self, name):  # pragma: no cover - interface
        raise NotImplementedError

    def readlink(self, path):  # pragma: no cover - interface
        raise NotImplementedError

    def now(self):  # pragma: no cover - interface
        raise NotImplementedError


def read_small_file(path, owners, limit=SMALL_FILE_LIMIT):
    """A small regular file that one of `owners` (uids) owns, read without
    following a link at its last component and without waiting: a FIFO,
    device, folder, link, foreign or oversized file raises OSError instead of
    blocking or filling memory. For files a less privileged account controls,
    such as the owner's preferences."""
    flags = os.O_RDONLY | os.O_NONBLOCK | getattr(os, "O_NOFOLLOW", 0) | getattr(os, "O_NOCTTY", 0)
    fd = os.open(path, flags | getattr(os, "O_CLOEXEC", 0))
    try:
        st = os.fstat(fd)
        if not stat.S_ISREG(st.st_mode):
            raise OSError(errno.EINVAL, "not a regular file", path)
        if st.st_uid not in owners:
            raise OSError(errno.EPERM, "not owned as expected", path)
        if st.st_size > limit:
            raise OSError(errno.EFBIG, "too large", path)
        data = bytearray()
        while len(data) <= limit:
            chunk = os.read(fd, limit + 1 - len(data))
            if not chunk:
                break
            data.extend(chunk)
        if len(data) > limit:
            raise OSError(errno.EFBIG, "too large", path)
        return bytes(data)
    finally:
        os.close(fd)


def resolve(name):
    """The absolute path of an allowed tool, from the system directories only."""
    if not isinstance(name, str) or "/" in name or name not in COMMANDS:
        raise CommandRefused(f"the engine doesn't run {name!r}")
    for directory in SYSTEM_PATH:
        path = os.path.join(directory, name)
        if os.path.isfile(path) and os.access(path, os.X_OK):
            return path
    raise CommandRefused(f"{name} isn't installed in the system directories")


class MacHost(Host):
    def run(self, argv, input=None, timeout=None):
        argv = [str(a) for a in argv]
        path = resolve(argv[0])
        log.debug("run: %s", " ".join(argv))
        try:
            p = subprocess.run(
                [path] + argv[1:],
                input=input,
                stdin=None if input is not None else subprocess.DEVNULL,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                env=dict(ENVIRONMENT),
                close_fds=True,
                timeout=timeout,
            )
        except subprocess.TimeoutExpired as error:
            return Completed(argv, -1, error.stdout or b"", (error.stderr or b"") + b"\n(timed out)")
        return Completed(argv, p.returncode, p.stdout, p.stderr)

    def open_raw(self, device, writable):
        if not _RAW_DEVICE.fullmatch(device):
            raise CommandRefused(f"not a partition: {device!r}")
        flags = (os.O_RDWR if writable else os.O_RDONLY) | getattr(os, "O_CLOEXEC", 0)
        fd = os.open("/dev/r" + device, flags)
        return os.fdopen(fd, "r+b" if writable else "rb", buffering=0)

    def flush_device(self, fileobj):
        """Ask the drive to put written data on stable storage. Returns how,
        for the record."""
        fd = fileobj.fileno()
        try:
            fcntl.fcntl(fd, F_FULLFSYNC)
            return "F_FULLFSYNC"
        except OSError:
            pass
        try:
            fcntl.ioctl(fd, DKIOCSYNCHRONIZECACHE)
            return "DKIOCSYNCHRONIZECACHE"
        except OSError:
            pass
        os.fsync(fd)
        return "fsync"

    def read_small_file(self, path, owners, limit=SMALL_FILE_LIMIT):
        return read_small_file(path, owners, limit)

    def user(self, name):
        """(uid, home folder) from the directory service, not a guessed
        /Users/NAME."""
        import pwd

        entry = pwd.getpwnam(name)
        return entry.pw_uid, entry.pw_dir

    def readlink(self, path):
        return os.readlink(path)

    def now(self):
        import time

        return int(time.time())


class SubprocessShim:
    """Stands in for the `subprocess` module inside Asahi's modules.

    Output is always captured (the engine's standard output is its
    transcript), and every command goes through the Host's list. Callers
    get what they asked for: output only where they asked for a pipe, and
    CalledProcessError with the tool's own words when `check` fails.
    """

    PIPE = subprocess.PIPE
    STDOUT = subprocess.STDOUT
    DEVNULL = subprocess.DEVNULL
    CalledProcessError = subprocess.CalledProcessError
    CompletedProcess = subprocess.CompletedProcess
    TimeoutExpired = subprocess.TimeoutExpired

    def __init__(self, host):
        self.host = host
        self.last = None

    def run(self, args, input=None, stdout=None, stderr=None, capture_output=False, check=False, **kwargs):
        if kwargs:
            raise CommandRefused(f"unexpected subprocess options {sorted(kwargs)}")
        if isinstance(args, (str, bytes)):
            raise CommandRefused("commands are argument lists, never shell strings")
        result = self.host.run(list(args), input=input)
        self.last = result
        if result.stdout:
            log.debug("%s: %s", args[0], result.stdout[-4096:])
        if result.stderr:
            log.debug("%s (stderr): %s", args[0], result.stderr[-4096:])
        want_out = capture_output or stdout == subprocess.PIPE
        want_err = capture_output or stderr == subprocess.PIPE
        if stderr == subprocess.STDOUT and want_out:
            out = (result.stdout or b"") + (result.stderr or b"")
        elif want_out:
            out = result.stdout or b""
        else:
            out = None
        err = (result.stderr or b"") if want_err else None
        if check and result.returncode != 0:
            raise subprocess.CalledProcessError(
                result.returncode,
                list(args),
                output=out if out is not None else result.output,
                stderr=err,
            )
        return subprocess.CompletedProcess(list(args), result.returncode, out, err)


def take_standard_streams():
    """Keep the transcript and acknowledgement channels for the engine alone.

    Returns private (transcript, acks) file objects. Afterwards standard
    output is standard error and standard input is empty, so neither Asahi's
    progress printing nor any child process can write into the transcript or
    read an acknowledgement.
    """
    transcript = os.fdopen(os.dup(1), "wb", buffering=0)
    acks = os.fdopen(os.dup(0), "rb", buffering=0)
    devnull = os.open(os.devnull, os.O_RDWR)
    os.dup2(2, 1)
    os.dup2(devnull, 0)
    os.close(devnull)
    sys.stdout = sys.stderr
    sys.stdin = open(os.devnull, "r")
    return transcript, acks


def read_password(fd=3, limit=1024):
    """Read the owner's password from `fd` once, to end of file, and close
    it. Returns a bytearray the caller wipes; None if the descriptor isn't
    open."""
    try:
        os.fstat(fd)
    except OSError:
        return None
    data = bytearray()
    try:
        while len(data) <= limit:
            chunk = os.read(fd, limit + 1 - len(data))
            if not chunk:
                break
            data.extend(chunk)
    finally:
        os.close(fd)
    return data


def wipe(buffer):
    """Overwrite a bytearray. Python may hold other copies (the bytes passed
    to a child's stdin); this clears the engine's own."""
    if isinstance(buffer, bytearray):
        for i in range(len(buffer)):
            buffer[i] = 0
        del buffer[:]
