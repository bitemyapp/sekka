# SPDX-License-Identifier: GPL-3.0-or-later
"""The engine's command line. Started by Sekka's privileged helper with the
pinned runtime, isolated (-I), from the extracted engine archive ROOT:

    ROOT/Frameworks/Python.framework/Versions/3.13/bin/python3.13 -I \
        ROOT/sekka_engine/__main__.py inspect
    … -I ROOT/sekka_engine/__main__.py execute --plan FILE --assets DIR \
        --installation UUID --owner NAME
    … -I ROOT/sekka_engine/__main__.py startup-disk --plan FILE \
        --installation UUID --owner NAME --esp-inventory DIGEST

`python3 -I -m sekka_engine …` runs the same code wherever the package is
importable; the archive doesn't make it importable from the runtime's
site-packages, because that directory is sealed by the runtime's code
signature.

`startup-disk` is the helper's bounded retry: only the startup-disk stage
again, for an installation execute left with its payload verified.
`--esp-inventory` is the esp-inventory fact of that execute run's Esp
checkpoint, from the helper's journal; the ESP must still match it.
`execute` and `startup-disk` read the owner's password from file
descriptor 3, to its end.
Standard output carries the transcript; standard input carries the helper's
`{"ack":SEQ}` answers; standard error carries a diagnostic log. There are
no other options: no simulation, no other installer, profile or command.
"""

import logging
import os
import sys


def usage(message):
    sys.stderr.write(f"sekka_engine: {message}\n")
    sys.stderr.write(
        "usage: python3 -I sekka_engine/__main__.py inspect\n"
        "       python3 -I sekka_engine/__main__.py execute --plan FILE --assets DIR"
        " --installation UUID --owner NAME   (password on fd 3)\n"
        "       python3 -I sekka_engine/__main__.py startup-disk --plan FILE"
        " --installation UUID --owner NAME --esp-inventory DIGEST   (password on fd 3)\n"
    )
    return 2


OPTIONS = {
    "execute": ("--plan", "--assets", "--installation", "--owner"),
    "startup-disk": ("--plan", "--installation", "--owner", "--esp-inventory"),
}


def parse(argv):
    if not argv or argv[0] not in ("inspect", "execute", "startup-disk"):
        raise ValueError("the mode is inspect, execute or startup-disk")
    mode, rest = argv[0], argv[1:]
    if mode == "inspect":
        if rest:
            raise ValueError("inspect takes no options")
        return mode, {}
    wanted = OPTIONS[mode]
    options = {}
    if len(rest) != 2 * len(wanted):
        raise ValueError(f"{mode} takes {', '.join(wanted)}, once each")
    for flag, value in zip(rest[::2], rest[1::2]):
        if flag not in wanted or flag in options or not value or value.startswith("-"):
            raise ValueError(f"unexpected {flag!r}")
        options[flag] = value
    return mode, options


def main(argv=None):
    argv = sys.argv[1:] if argv is None else argv
    # Before anything else can write to standard output or read standard
    # input, keep them for the transcript and the acknowledgements.
    from .host import MacHost, read_password, take_standard_streams, wipe

    try:
        mode, options = parse(argv)
    except ValueError as error:
        return usage(str(error))
    password = read_password(3) if mode in OPTIONS else None
    transcript, acks = take_standard_streams()
    logging.basicConfig(stream=sys.stderr, level=logging.INFO, format="%(asctime)s %(name)s %(levelname)s %(message)s")

    from . import provenance
    from .asahi import load
    from .engine import Engine

    root = os.path.dirname(provenance.package_dir())
    # A fixed environment: the helper's is already fixed; this makes sure.
    os.environ.clear()
    os.environ.update(
        {
            "PATH": "/usr/bin:/bin:/usr/sbin:/sbin",
            "LC_ALL": "C",
            "LANG": "C",
            # Asahi's URLCache uses Python's default TLS context: the pinned
            # runtime's CA bundle, as Asahi's install.sh sets it.
            "SSL_CERT_FILE": os.path.join(
                root, "Frameworks/Python.framework/Versions/Current/etc/openssl/cert.pem"
            ),
        }
    )
    adapter = provenance.adapter_digest()
    host = MacHost()
    try:
        asahi = load(root, host)
        lock = _read_lock(root, adapter)
        with open(os.path.join(root, provenance.PROFILE_DIR, lock["profile"]["file"]), "rb") as f:
            profile_bytes = f.read()
        from . import contract

        profile, digest = contract.load_profile(profile_bytes)
        if digest != lock["profile"]["digest"]:
            raise RuntimeError("the bundled hardware profile isn't the one the engine was built with")
    except Exception as error:
        # The engine itself is damaged or incomplete: report it in the
        # protocol's own terms rather than not at all.
        from . import ASAHI_VERSION, ENGINE_VERSION, contract
        from .transcript import TranscriptWriter

        t = TranscriptWriter(transcript)
        t.hello(ENGINE_VERSION, ASAHI_VERSION, adapter, mode)
        t.failure(None, contract.failure("engine-damaged", f"The installer's engine can't start: {error}", None, "no"))
        t.completion("failed")
        wipe(password)
        return 1
    engine = Engine(
        host=host,
        asahi=asahi,
        profile_bytes=profile_bytes,
        out=transcript,
        acks=acks,
        adapter=adapter,
        password=password,
    )
    if mode == "inspect":
        outcome = engine.run_inspect()
    else:
        try:
            with open(options["--plan"], "rb") as f:
                plan_bytes = f.read(64 * 1024 + 1)
        except OSError as error:
            logging.error("the plan can't be read: %s", error)
            plan_bytes = b""  # refused as unreadable, through the transcript
        if mode == "execute":
            outcome = engine.run_execute(plan_bytes, options["--assets"], options["--installation"], options["--owner"])
        else:
            outcome = engine.run_startup_disk(
                plan_bytes, options["--installation"], options["--owner"], options["--esp-inventory"]
            )
    return 0 if outcome in ("inspected", "waiting-for-recovery") else 1


def _read_lock(root, adapter):
    import json

    from . import ASAHI_VERSION, ENGINE_VERSION
    from . import provenance

    with open(os.path.join(root, provenance.LOCK_FILE), "rb") as f:
        lock = json.loads(f.read())
    if lock.get("schema") != provenance.LOCK_SCHEMA:
        raise RuntimeError("the engine lock has an unknown schema")
    if lock.get("engine") != ENGINE_VERSION or lock.get("asahi", {}).get("version") != ASAHI_VERSION:
        raise RuntimeError("the engine lock names another version")
    if lock.get("adapter", {}).get("digest") != adapter:
        raise RuntimeError("the adapter's files aren't the ones the engine was built with")
    return lock


if __name__ == "__main__":
    if not __package__:
        # Run by path: -I keeps this file's directory off sys.path, so add
        # the archive's root (which holds this package) and nothing else.
        sys.path.append(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
        from sekka_engine.__main__ import main as package_main

        sys.exit(package_main())
    sys.exit(main())
