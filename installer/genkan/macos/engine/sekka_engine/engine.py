# SPDX-License-Identifier: GPL-3.0-or-later
"""The three modes: `inspect` describes the Mac; `execute` carries out one
plan; `startup-disk` runs that plan's startup-disk stage again.

`execute` refuses before anything changes unless the plan, the assets, the
hardware profile and Asahi's own view of the Mac all agree. It then runs the
stages of engine.rs `macos_stages` in order. Before each it writes an Intent
and waits for the helper's acknowledgement; after each it writes a
Checkpoint with what it found and how far that was verified. The first
failure ends the run: a mutating stage is never retried here.

`startup-disk` is the helper's bounded retry after that stage failed. It
finds the installation by the identity in its Recovery configuration, checks
that what execute verified is still in place, and runs the same
stage_startup_disk, and nothing else, under the same protocol.

Parts adapted from Omarchy's engine overlay
(omacom/omarchy-mac-installer c07e54c2134a8ec1415e7e2b917589ae1d3d7dc9,
Engine/overlay/src/omarchy_asahi.py and omarchy_recovery.py, MIT,
Copyright (c) David Heinemeier Hansson) are marked where they appear.
"""

import hashlib
import json
import logging
import os
import plistlib
import re
import shutil
import signal
import stat
import subprocess
import tempfile

from . import ENGINE_VERSION, ASAHI_VERSION
from . import apple_inputs
from . import assets as A
from . import canonical
from . import contract as c
from .hints import read_hints
from .host import wipe
from .inspect import RECOVERY_APP, inspect_mac
from .transcript import AckError, TranscriptWriter

log = logging.getLogger("sekka_engine")

CHUNK = 8 * 1024 * 1024
PROGRESS_STEP = 64 * 1024 * 1024
OWNER = re.compile(r"[A-Za-z0-9_][A-Za-z0-9._-]{0,254}\Z")
# macOS's own bookkeeping on a mounted FAT volume, not part of the ESP.
MACOS_BOOKKEEPING = (".fseventsd", ".Spotlight-V100", ".Trashes", ".TemporaryItems")
LAYOUT_STAGES = ("shrink", "stub-partition", "partitions")


class Refusal(Exception):
    """The plan can't be carried out on this Mac; nothing was changed."""

    def __init__(self, code, message, diagnostic=None):
        super().__init__(message)
        self.code = code
        self.message = message
        self.diagnostic = diagnostic


class Terminated(BaseException):
    """Asked to stop (SIGTERM: the helper cancelling, or its inspection
    taking too long), where nothing is changing the disk. Raised where the
    run is, so that its own cleanup (unmounting what it mounted, deleting
    its working folder and Apple's files in it) runs before it exits. Not an
    Exception: nothing that reports a stage's errors catches it, and nothing
    more is written to the transcript."""


class StageFailure(Exception):
    def __init__(self, code, message, diagnostic=None):
        super().__init__(message)
        self.code = code
        self.message = message
        self.diagnostic = diagnostic


def called_process_diagnostic(error):
    """Apple's own words and exit code from a failed tool, verbatim."""
    output = error.output or b""
    if isinstance(output, str):
        output = output.encode("utf-8", "replace")
    if error.stderr and error.stderr not in output:
        output += error.stderr if isinstance(error.stderr, bytes) else error.stderr.encode()
    command = " ".join(str(a) for a in (error.cmd[:3] if isinstance(error.cmd, list) else [error.cmd]))
    text = output.decode("utf-8", "replace").strip()
    return f"{text}\n({command} exited with status {error.returncode})".strip()


def walk_files(root, skip=()):
    """Every regular file under `root`, as relative paths, refusing links."""
    out = {}
    for dirpath, dirnames, filenames in os.walk(root):
        rel_dir = os.path.relpath(dirpath, root)
        if rel_dir == ".":
            dirnames[:] = [d for d in dirnames if d not in skip]
        for name in dirnames + filenames:
            path = os.path.join(dirpath, name)
            if os.path.islink(path):
                raise StageFailure("unexpected-link", f"{os.path.relpath(path, root)} is a link")
        for name in filenames:
            path = os.path.join(dirpath, name)
            rel = os.path.normpath(os.path.join(rel_dir, name)).replace(os.sep, "/")
            out[rel] = path
    return out


APPLEDOUBLE_MAGIC = b"\x00\x05\x16\x07"


def is_appledouble_sidecar(path):
    """True only for a real AppleDouble file (by its header) whose companion
    exists; any other `._` file stays and fails the read-back. Adapted from
    Omarchy's omarchy_recovery.is_appledouble_sidecar (MIT, see the module
    docstring)."""
    name = os.path.basename(path)
    if not name.startswith("._") or len(name) <= 2 or name == "._..":
        return False
    if os.path.islink(path) or not os.path.isfile(path):
        return False
    companion = os.path.dirname(path) if name == "._." else os.path.join(os.path.dirname(path), name[2:])
    if not os.path.lexists(companion):
        return False
    try:
        with open(path, "rb") as f:
            return f.read(len(APPLEDOUBLE_MAGIC)) == APPLEDOUBLE_MAGIC
    except OSError:
        return False


def remove_appledouble_sidecars(root):
    """Delete the AppleDouble files macOS writes on FAT volumes for extended
    attributes FAT can't hold; nothing that boots from the ESP reads them.
    Adapted from Omarchy's omarchy_recovery.remove_appledouble_sidecars (MIT,
    see the module docstring)."""
    removed = 0
    for dirpath, dirnames, filenames in os.walk(root):
        for name in filenames:
            path = os.path.join(dirpath, name)
            if is_appledouble_sidecar(path):
                os.unlink(path)
                removed += 1
    return removed


class Engine:
    """One run. Everything it touches is passed in: `host` (commands, raw
    devices), `asahi` (the pinned modules, already routed through `host`),
    the bundled profile's bytes, and the transcript's two channels."""

    def __init__(
        self, *, host, asahi, profile_bytes, out, acks=None, adapter, workdir=None, password=None, privileged=None
    ):
        self.host = host
        self.privileged = os.geteuid() == 0 if privileged is None else privileged
        self.asahi = asahi
        self.profile_bytes = profile_bytes
        self.t = TranscriptWriter(out, acks)
        self.adapter = adapter
        self.workdir = workdir
        self.password = password
        self.changed = False
        self.progress_state = {}
        self.view = None
        self.working = None
        # Sekka's own volumes, which only the stages mount read-write: the
        # planned stub's container once it exists, and the planned ESP.
        self.writable_container = None
        self.writable_esp = None
        # A stage that changes the disk is running, and a stop was asked
        # for during one.
        self.changing_disk = False
        self.stop_asked = False

    def handle_sigterm(self):
        """Stop on SIGTERM through the run's own cleanup (see Terminated);
        returns the handler it replaced."""
        return signal.signal(signal.SIGTERM, self.on_sigterm)

    def on_sigterm(self, signum, frame):
        if self.changing_disk:
            # Apple's tools are never stopped part way (subprocess.run would
            # kill the one running): the stage finishes, and the run stops
            # before the next.
            log.warning("asked to stop during the %s stage; stopping once it ends", self.stage)
            self.stop_asked = True
            return
        # A second request while cleaning up stops at once.
        signal.signal(signal.SIGTERM, signal.SIG_DFL)
        raise Terminated()

    # inspect -----------------------------------------------------------------

    def run_inspect(self, owner=None):
        """Describe the Mac. With `owner` (the helper's inspection before it
        opens an installation), first check that account and the password
        on descriptor 3 by the rules execute and startup-disk apply, so the
        helper refuses what they would before anything is recorded or
        imported."""
        self.t.hello(ENGINE_VERSION, ASAHI_VERSION, self.adapter, "inspect")
        try:
            try:
                if owner is not None:
                    self.check_owner(owner)
                profile, _ = c.load_profile(self.profile_bytes)
                doc, inspector = inspect_mac(self.asahi, self.host, profile, privileged=self.privileged)
                # Without the running macOS the inspection says so in an issue
                # that stops installing; there is no list to look in.
                if owner is not None and inspector.cur_os is not None:
                    self.check_admin(inspector.cur_os)
            except Refusal as refusal:
                self.t.failure(None, c.failure(refusal.code, refusal.message, refusal.diagnostic, "no"))
                self.t.completion("failed")
                return "failed"
            except Exception as error:  # report every failure through the transcript
                log.exception("inspection failed")
                self.t.failure(
                    None,
                    c.failure(
                        "inspection-failed",
                        f"This Mac couldn't be inspected: {error}",
                        None,
                        "no",
                    ),
                )
                self.t.completion("failed")
                return "failed"
        finally:
            wipe(self.password)
        for what in inspector.unavailable:
            self.t.log("info", f"Not read without administrator rights: {what}.")
        self.t.inspection(doc)
        self.t.completion("inspected")
        return "inspected"

    # execute -----------------------------------------------------------------

    def run_execute(self, plan_bytes, assets_dir, installation, owner):
        self.t.hello(ENGINE_VERSION, ASAHI_VERSION, self.adapter, "execute")
        previous = os.getcwd()
        made_workdir = False
        try:
            try:
                if self.workdir is None:
                    self.workdir = tempfile.mkdtemp(prefix="sekka-engine-")
                    made_workdir = True
                self.prepare(plan_bytes, assets_dir, installation, owner)
            except Refusal as refusal:
                self.t.failure(None, c.failure(refusal.code, refusal.message, refusal.diagnostic, "no"))
                self.t.completion("failed")
                return "failed"
            except Exception as error:  # nothing has changed yet; say so in the protocol
                log.exception("preparing to execute failed")
                self.t.failure(None, c.failure("internal-error", f"The installation couldn't start: {error}", None, "no"))
                self.t.completion("failed")
                return "failed"
            os.chdir(self.workdir)
            return self.run_stages()
        finally:
            os.chdir(previous)
            wipe(self.password)
            self.close_working_view()
            for name in ("payload", "recovery_zip"):
                archive = getattr(self, name, None)
                if archive is not None:
                    A.close_zip(archive)
            if made_workdir:
                shutil.rmtree(self.workdir, ignore_errors=True)

    def run_stages(self, stages=None):
        if stages is None:
            stages = c.macos_stages(self.shrink is not None)
        for stage in stages:
            if self.stop_asked:
                # Asked during the last stage: nothing more starts.
                raise Terminated()
            seq = self.t.intent(stage, self.plan_digest)
            try:
                self.t.wait_ack(seq)
            except AckError as error:
                self.t.failure(
                    stage,
                    c.failure(
                        "not-acknowledged",
                        f"The helper didn't acknowledge the {stage} stage, so it didn't start.",
                        str(error),
                        "yes" if self.changed else "no",
                    ),
                )
                self.t.completion("failed")
                return "failed"
            self.stage = stage
            self.changing_disk = stage != "apple-inputs"
            try:
                facts, verification = getattr(self, "stage_" + stage.replace("-", "_"))()
            except StageFailure as failure:
                self.fail(stage, failure.code, failure.message, failure.diagnostic)
                return "failed"
            except subprocess.CalledProcessError as error:
                self.fail(
                    stage,
                    f"{stage}-failed",
                    f"An Apple tool failed during the {stage} stage.",
                    called_process_diagnostic(error),
                )
                return "failed"
            except Exception as error:
                log.exception("stage %s failed", stage)
                self.fail(stage, "internal-error", f"The {stage} stage stopped: {error}", None)
                return "failed"
            finally:
                self.changing_disk = False
            self.t.checkpoint(stage, self.plan_digest, facts, verification)
            if stage != "apple-inputs":
                self.changed = True
        self.t.completion("waiting-for-recovery")
        return "waiting-for-recovery"

    def fail(self, stage, code, message, diagnostic):
        changed = self.disk_changed(stage)
        self.t.failure(stage, c.failure(code, message, diagnostic, changed))
        self.t.completion("failed")

    def disk_changed(self, stage):
        """Whether the disk differs from the one the plan was made for. Once
        a stage has changed it, it has. Before that, look again."""
        if self.changed:
            return "yes"
        if stage == "apple-inputs":
            return "no"
        try:
            doc, _ = inspect_mac(self.asahi, self.host, self.profile, privileged=True)
        except Exception as error:
            log.warning("reinspection after the failure failed: %s", error)
            return "unknown"
        same = doc["layout_digest"] == self.plan["disk"]["layout_digest"]
        self.t.log(
            "info",
            f"After the failure the disk's layout {'is unchanged' if same else 'differs from the plan'} "
            f"({doc['layout_digest']}).",
        )
        return "no" if same else "yes"

    # startup-disk ----------------------------------------------------------------

    def run_startup_disk(self, plan_bytes, installation, owner, esp_inventory):
        """The helper's bounded retry: the startup-disk stage again, and
        nothing else, for an installation whose execute run verified its
        payload and then failed to set the startup disk. `esp_inventory` is
        the esp-inventory fact of that run's Esp checkpoint, from the
        helper's journal."""
        self.t.hello(ENGINE_VERSION, ASAHI_VERSION, self.adapter, "startup-disk")
        try:
            try:
                self.prepare_retry(plan_bytes, installation, owner, esp_inventory)
            except Refusal as refusal:
                self.t.failure(None, c.failure(refusal.code, refusal.message, refusal.diagnostic, "no"))
                self.t.completion("failed")
                return "failed"
            except Exception as error:  # nothing has changed yet; say so in the protocol
                log.exception("preparing the startup-disk retry failed")
                self.t.failure(None, c.failure("internal-error", f"The retry couldn't start: {error}", None, "no"))
                self.t.completion("failed")
                return "failed"
            # The installation changed the disk before this run; a failure
            # now leaves it changed, whatever this stage did.
            self.changed = True
            return self.run_stages(["startup-disk"])
        finally:
            wipe(self.password)
            self.close_working_view()

    def prepare_retry(self, plan_bytes, installation, owner, esp_inventory):
        """Find this installation and check it is still what execute left:
        the same Mac, disk and macOS, its stub, ESP and root by identity, the
        Recovery data and handoff it wrote, and the ESP's whole inventory as
        the journal recorded it. The layout digest isn't compared: the
        installation's own partitions changed it."""
        self.check_request(plan_bytes, installation, owner)
        try:
            self.esp_inventory = c.digest(esp_inventory)
        except c.Invalid:
            raise Refusal("esp-inventory-invalid", "The ESP inventory to check against isn't a SHA-256 digest.")
        plan = self.plan
        try:
            doc, inspector = inspect_mac(self.asahi, self.host, self.profile, privileged=True)
        except Exception as error:
            log.exception("inspection before the retry failed")
            raise Refusal("inspection-failed", f"This Mac couldn't be inspected: {error}")
        self.inspection, self.inspector, self.view = doc, inspector, inspector.view
        m, pm = doc["machine"], plan["machine"]
        if m["binding"] != pm["binding"] or any(
            m[k] != pm[k] for k in ("product_type", "device_class", "chip_id", "system_firmware")
        ):
            raise Refusal("wrong-machine", "This plan was made for another Mac, or this Mac's firmware changed.")
        if doc["disk"]["uuid"] != plan["disk"]["disk"]:
            raise Refusal("wrong-disk", "This plan was made for another disk.")
        if doc["macos"]["volume_group"] != plan["disk"]["macos_volume_group"]:
            raise Refusal("wrong-macos", "The plan was made from a different macOS on this Mac.")
        normal = [i for i in doc["issues"] if i["code"] == "not-normal-boot"]
        if normal or inspector.cur_os is None:
            raise Refusal(
                "not-normal-boot",
                normal[0]["detail"] if normal else "The running macOS can't be found on the internal disk.",
            )
        self.cur_os = inspector.cur_os
        self.check_admin(self.cur_os)
        # Looked at read-only until the checks pass; then only the stub's own
        # volumes become writable (the stage renames its SystemVersion).
        self.open_working_view()
        p, o, config = self.find_installation()
        self.check_installation(p, o, config)
        self.stub_uuid = c.uuid_any(p.uuid)
        self.stub_container = config["stub"]["container"]
        self.stub = dict(config["stub"])
        self.esp_uuid, self.root_uuid = config["esp"], config["root"]
        self.writable_container = self.stub_container
        v = self.view
        self.ins = self.asahi.stub.StubInstaller(v.sysinfo, v.dutil, v.osinfo)
        self.t.log("info", f"Setting the startup disk again for installation {self.installation} on {o.label}.")

    def find_installation(self):
        """The one stub whose Sekka/InstallData/recovery.json names this
        installation."""
        found = []
        for p, o in self.view.oses():
            if not o.stub or not o.system:
                continue
            path = os.path.join(o.system, c.RECOVERY_DATA_DIR, c.RECOVERY_CONFIG_FILE)
            try:
                with open(path, "rb") as f:
                    raw = f.read(c.LIMIT_RECOVERY + 1)
            except OSError:
                continue
            try:
                config = c.parse_recovery_config(raw)
            except (c.Invalid, canonical.CanonicalError, KeyError, TypeError) as error:
                log.info("%s: unreadable Sekka Recovery configuration: %s", o.label, error)
                continue
            if config["installation"] == self.installation:
                found.append((p, o, config))
        if not found:
            raise Refusal(
                "installation-not-found",
                f"No startup volume on this Mac holds installation {self.installation}.",
            )
        if len(found) > 1:
            raise Refusal(
                "installation-ambiguous",
                f"More than one startup volume claims installation {self.installation}; nothing was changed.",
            )
        return found[0]

    def check_installation(self, p, o, config):
        """Every identity the Recovery configuration names is on the disk, and
        what execute wrote and verified is still there, unchanged."""
        plan = self.plan

        def changed(what):
            raise Refusal(
                "installation-changed",
                f"This installation isn't as it was left: {what}. Setting the startup disk again isn't safe.",
            )

        if config["release"] != plan["release"]["release"] or config["volume_name"] != plan["volume_name"]:
            changed("its Recovery configuration names another release")
        if config["owner"]["macos_volume_group"] != plan["disk"]["macos_volume_group"]:
            changed("its Recovery configuration names another macOS")
        stub = config["stub"]
        if p.container is None or c.uuid_any(p.container["APFSContainerUUID"]) != stub["container"]:
            changed("its startup volume's container differs")
        if c.uuid_any(o.vgid) != stub["volume_group"]:
            changed("its startup volume group differs")
        volumes = {x["DeviceIdentifier"]: x for x in p.container.get("Volumes", [])}
        for device, want in ((o.sys_volume, stub["system_volume"]), (o.data_volume, stub["data_volume"])):
            if device not in volumes or c.uuid_any(volumes[device]["APFSVolumeUUID"]) != want:
                changed("its startup volume's System or Data volume differs")
        esp = self.view.partition(config["esp"])
        root = self.view.partition(config["root"])
        if esp is None or esp.type != "EFI":
            changed("its ESP is missing")
        if root is None or root.type != "Linux Filesystem":
            changed("its root partition is missing")
        data = os.path.join(o.system, c.RECOVERY_DATA_DIR)
        for item in [config["boot_object"]] + config["firmware"]:
            path = os.path.join(data, *A.check_relative(item["path"]))
            try:
                found = c.sha256_file(path)
            except OSError:
                changed(f"{item['path']} is missing")
            if found != (item["size"], item["digest"]):
                changed(f"{item['path']} differs from its record")
        if not os.path.isfile(os.path.join(o.system, RECOVERY_APP, "Contents", "Info.plist")):
            changed("the Recovery app is missing")
        try:
            with open(os.path.join(o.system, ".IAPhysicalMedia"), "rb") as f:
                media = plistlib.load(f)
        except (OSError, plistlib.InvalidFileException, ValueError):
            media = None
        if not isinstance(media, dict) or media.get("AppName") != RECOVERY_APP:
            changed("the startup volume doesn't open the Recovery app")
        files, handoff, entries = self.read_esp(esp)
        if files is None:
            changed("its ESP can't be read")
        if A.esp_inventory_digest(files) != self.esp_inventory:
            changed("its ESP's files differ from what was installed and verified")
        if handoff is None or (handoff["installation"], handoff["root"], handoff["esp"]) != (
            self.installation,
            config["root"],
            config["esp"],
        ):
            changed("its ESP's handoff names another installation")
        if not entries or any(c.ROOT_PLACEHOLDER in e or c.ESP_PLACEHOLDER in e for e in entries.values()):
            changed("its ESP's boot entries aren't filled in")
        # By the rule execute filled them by, and first-boot setup reads them.
        if not any(c.names_partitions(e, config["root"], config["esp"]) for e in entries.values()):
            changed("no boot entry on its ESP names this installation's partitions")

    def read_esp(self, esp):
        """The ESP's whole inventory, its handoff and its boot entries, read
        through a read-only mount that is undone afterwards. (None, None,
        None) if it can't be read."""
        dutil = self.view.dutil
        try:
            mount = dutil.mount_read_only(esp.name)
        except (subprocess.CalledProcessError, OSError, KeyError) as error:
            log.warning("the ESP couldn't be mounted read-only: %s", error)
            return None, None, None
        try:
            files = []
            for rel, path in walk_files(mount, skip=MACOS_BOOKKEEPING).items():
                size, digest = c.sha256_file(path)
                files.append({"path": rel, "size": size, "digest": digest})
            try:
                with open(os.path.join(mount, c.HANDOFF_PATH), "rb") as f:
                    handoff = c.parse_handoff(f.read(c.LIMIT_HANDOFF + 1))
            except (OSError, c.Invalid, canonical.CanonicalError, KeyError, TypeError):
                handoff = None
            entries = {}
            folder = os.path.join(mount, "loader", "entries")
            for name in sorted(os.listdir(folder)) if os.path.isdir(folder) else []:
                if name.endswith(".conf"):
                    with open(os.path.join(folder, name), "rb") as f:
                        entries[name] = f.read(64 * 1024).decode("utf-8", "replace")
            return files, handoff, entries
        except (StageFailure, OSError) as error:
            log.warning("the ESP couldn't be read: %s", error)
            return None, None, None
        finally:
            try:
                dutil.unmount(esp.name)
            except Exception as error:
                log.warning("couldn't unmount the ESP again: %s", error)

    # Before anything changes ---------------------------------------------------

    def check_request(self, plan_bytes, installation, owner):
        """What execute and startup-disk both check first: the plan, the
        installation's identity, the owner and password, and the profile."""
        try:
            self.plan, self.plan_digest = c.parse_plan(plan_bytes)
        except (c.Invalid, canonical.CanonicalError, KeyError, TypeError) as error:
            raise Refusal("plan-invalid", f"The plan can't be read: {error}")
        plan = self.plan
        self.shrink = c.plan_shrink(plan)
        try:
            self.installation = c.uuid(installation)
        except c.Invalid:
            raise Refusal("installation-invalid", "The installation identity isn't a lower-case UUID.")
        self.check_owner(owner)
        try:
            self.profile, profile_digest = c.load_profile(self.profile_bytes)
        except (c.Invalid, canonical.CanonicalError, KeyError, TypeError) as error:
            raise Refusal("profile-invalid", f"The engine's hardware profile can't be read: {error}")
        if profile_digest != plan["release"]["profile_digest"] or self.profile["id"] != plan["release"]["profile"]:
            raise Refusal(
                "profile-mismatch",
                "The plan was made for a different hardware profile than the one this engine carries.",
            )

    def prepare(self, plan_bytes, assets_dir, installation, owner):
        self.check_request(plan_bytes, installation, owner)
        plan = self.plan
        util = self.asahi.util
        try:
            paths = A.verify_assets(assets_dir, plan)
            self.installer_data, self.template = A.load_template(
                paths["installer_data"], util, plan, self.profile["firmware"]["os_firmware"][0]
            )
            self.manifest, self.esp_files = A.load_manifest(paths["manifest"], plan)
            self.payload = A.check_payload(paths["payload"], self.manifest, self.esp_files)
            self.recovery_zip, self.recovery_members = A.check_recovery_zip(paths["recovery_app"])
        except (A.AssetError, c.Invalid, OSError, ValueError, KeyError) as error:
            raise Refusal("asset-mismatch", f"The release's files can't be used: {error}")
        try:
            doc, inspector = inspect_mac(self.asahi, self.host, self.profile, privileged=True)
        except Exception as error:
            log.exception("inspection before execution failed")
            raise Refusal("inspection-failed", f"This Mac couldn't be inspected: {error}")
        self.inspection, self.inspector, self.view = doc, inspector, inspector.view
        self.t.log("info", f"Asahi sees the layout {doc['layout_digest']}.")
        self.check_against_plan(doc, inspector)
        self.check_actions(doc)
        self.cur_os = inspector.cur_os
        self.check_admin(self.cur_os)
        self.ipsw = self.choose_ipsw()
        self.t.log("info", f"Asahi's installer gives the new system macOS {self.ipsw.version}'s OS firmware.")
        # Room for Apple's files, which the apple-inputs stage keeps in the
        # macOS container until the run ends: above Asahi's minimum when the
        # container shrinks, otherwise its free space less the margin a
        # free-space plan keeps there (plan.rs MACOS_MARGIN).
        if self.shrink:
            self.apple_room = max(0, self.shrink["to"] - self.shrink_minimum)
        else:
            macos = [k for k in doc["disk"]["containers"] if k["partition"] == c.uuid_any(self.cur_os.partition.uuid)]
            self.apple_room = max(0, macos[0]["free"] - c.MACOS_MARGIN) if macos else 0
        # The setup hints are read now, before any intent, from the owner's
        # own files: if reading them fails or stalls, nothing has changed.
        self.full_name = self.owner_full_name()
        self.hints = read_hints(self.host, self.owner, self.full_name)
        for name in ("boot", "logo.icns", "step2"):
            link = os.path.join(self.workdir, name)
            if not os.path.lexists(link):
                os.symlink(os.path.join(self.asahi.root, name), link)
        self.open_working_view()

    def owner_full_name(self):
        """The owner's full name from the running macOS's
        AdminUserRecoveryInfo.plist (the record Asahi copies to the stub),
        read as a small root-owned file; None if it can't be read."""
        o = self.cur_os
        try:
            path = os.path.join(o.preboot, o.vgid, "var/db/AdminUserRecoveryInfo.plist")
            records = plistlib.loads(self.host.read_small_file(path, {0}))
            name = records[self.owner]["RealName"]
        except Exception as error:
            log.info("no full name for %s: %s", self.owner, error)
            return None
        return name if isinstance(name, str) else None

    # The owner, as every mode that is given one checks it ------------------------
    #
    # The refusals' codes are contract.OWNER_REFUSALS: the helper tells the
    # owner these are theirs to change, not the Mac's.

    def check_owner(self, owner):
        """The owner's short name and the password on descriptor 3."""
        if not isinstance(owner, str) or not OWNER.fullmatch(owner):
            raise Refusal("owner-invalid", "The owner's account name isn't a macOS short name.")
        self.owner = owner
        self.check_password()

    def check_admin(self, cur_os):
        """The owner can approve the new system's startup in Recovery: Asahi's
        admin_users, the running macOS's AdminUserRecoveryInfo.plist, which
        the stub's Recovery gets a copy of."""
        admins = cur_os.admin_users or []
        if self.owner not in admins or self.owner == "_mbsetupuser":
            raise Refusal(
                "owner-not-admin",
                f"{self.owner} isn't one of the administrators this Mac's Recovery accepts. macOS keeps that "
                "list on its Preboot volume and brings it up to date when macOS updates, so an account made an "
                "administrator or given a secure token since then isn't on it yet; "
                "“diskutil apfs updatePreboot /” in Terminal brings it up to date now.",
            )

    def check_password(self):
        p = self.password
        if p is None:
            raise Refusal("no-password", "The owner's password wasn't given on file descriptor 3.")
        if not 0 < len(p) <= 1024:
            raise Refusal("password-unusable", "The owner's password is empty or longer than 1024 bytes.")
        try:
            decoded = bytes(p).decode("utf-8")
        except UnicodeDecodeError:
            raise Refusal("password-unusable", "The owner's password isn't UTF-8 text.")
        if any(ch in decoded for ch in "\n\r\0"):
            raise Refusal("password-unusable", "The owner's password contains a line break.")
        if decoded != decoded.strip():
            # bless --stdinpass can't take such a password (Asahi's bless()).
            raise Refusal(
                "password-unusable",
                "The owner's password starts or ends with a space, which Apple's bless can't accept "
                "from an installer. Use another owner of this Mac, or change this account's password "
                "so that it doesn't.",
            )
        del decoded

    def check_against_plan(self, doc, inspector):
        plan = self.plan
        m, pm = doc["machine"], plan["machine"]
        if m["binding"] != pm["binding"] or any(
            m[k] != pm[k] for k in ("product_type", "device_class", "chip_id", "system_firmware")
        ):
            raise Refusal("wrong-machine", "This plan was made for another Mac, or this Mac's firmware changed.")
        if doc["disk"]["uuid"] != plan["disk"]["disk"]:
            raise Refusal("wrong-disk", "This plan was made for another disk.")
        if doc["layout_digest"] != plan["disk"]["layout_digest"]:
            raise Refusal(
                "layout-changed",
                "This Mac's disk has changed since the plan was made. Check this Mac again and review a "
                "new plan.",
            )
        if doc["macos"]["volume_group"] != plan["disk"]["macos_volume_group"]:
            raise Refusal("wrong-macos", "The plan was made from a different macOS on this Mac.")
        refusals, acks = inspector.admission
        if refusals:
            raise Refusal(
                "not-admitted",
                "This Mac isn't one this release supports: " + ", ".join(kind for kind, _ in refusals) + ".",
            )
        needed = set()
        for kind, _ in acks:
            needed.add({"untested-system-firmware": "untested-system-firmware", "known-issue": "known-macos-issue"}[kind])
        if not needed <= set(plan["acknowledgements"]):
            raise Refusal(
                "acknowledgement-missing",
                "The plan's review didn't include an acknowledgement this Mac needs.",
            )
        blocking = [i for i in doc["issues"] if i["blocking"]]
        if blocking:
            raise Refusal("blocked", " ".join(i["detail"] for i in blocking))

    def check_actions(self, doc):
        """Every action against Asahi's own arithmetic and the live layout."""
        plan = self.plan
        main = self.asahi.main
        a, target = plan["allocation"], plan["target"]
        end = target["offset"] + target["size"]
        if a["stub"] != main.STUB_SIZE:
            raise Refusal("plan-mismatch", "The plan's stub isn't the size Asahi's installer makes.")
        minimum = main.STUB_SIZE + sum(
            self.asahi.util.align_up(self.asahi.util.psize(part["size"]), c.ALIGN) for part in self.template["partitions"]
        )
        if a["sekka"] < minimum:
            raise Refusal("plan-mismatch", "The plan gives Sekka less than Asahi's minimum for this OS.")
        v = self.view
        if self.shrink:
            s = self.shrink
            containers = [k for k in doc["disk"]["containers"] if k["uuid"] == s["container"]]
            candidates = [
                k for k in doc["candidates"] if k["kind"] == "shrink-container" and k["container"] == s["container"]
            ]
            if not containers or not candidates:
                raise Refusal("place-gone", "Asahi no longer offers to shrink that container.")
            k, cand = containers[0], candidates[0]
            part = v.partition(k["partition"])
            if s["from"] != k["size"] or part is None or part.size != s["from"]:
                raise Refusal("plan-mismatch", "The container isn't the size the plan shrinks it from.")
            if k["resize"] is None or s["to"] < k["resize"]["minimum"]:
                raise Refusal(
                    "shrink-too-far",
                    "The plan shrinks macOS below the smallest size Asahi's installer allows "
                    "(macOS keeps 38 GB free for its updates).",
                )
            self.shrink_minimum = k["resize"]["minimum"]
            if part.offset + s["to"] != target["offset"] or end > part.offset + part.size + cand["adjacent_free"]:
                raise Refusal("plan-mismatch", "Sekka's place doesn't follow the shrunk container.")
            self.shrink_partition = part.uuid
        else:
            covering = [p for p in v.parts if p.free and p.offset <= target["offset"] and p.offset + p.size >= end]
            if not covering or covering[0].size < main.STUB_SIZE:
                raise Refusal("place-gone", "The free space the plan uses isn't free.")
            self.shrink_partition = None

    def choose_ipsw(self):
        """InstallerMain.choose_ipsw without its prompts: the newest image
        this Mac's firmware and the release's installer data allow."""
        a = self.asahi
        util, main = a.util, a.main
        s = self.view.sysinfo
        supported = self.template.get("supported_fw")
        device = main.DEVICES[s.device_class]
        floor = max(util.split_ver(main.CHIP_MIN_VER.get(s.chip_id, "0")), util.split_ver(device.min_ver))
        candidates = [
            i
            for i in main.IPSW_VERSIONS
            if util.split_ver(i.version) >= floor
            and (supported is None or i.version in supported)
            and (i.devices is None or s.device_class in i.devices)
        ]
        avail = [
            i
            for i in candidates
            if util.split_ver(i.min_iboot) <= util.split_ver(s.sys_firmware)
            and util.split_ver(i.min_macos) <= util.split_ver(s.macos_ver)
            and util.split_ver(i.min_sfr) <= util.split_ver(s.sfr_full_ver or "0")
            and not i.expert_only
        ]
        if not avail:
            raise Refusal("os-firmware", "This Mac's system firmware is too old for the OS firmware Sekka needs.")
        chosen = avail[-1]
        if chosen.version not in self.profile["firmware"]["os_firmware"]:
            raise Refusal(
                "os-firmware",
                f"Asahi's installer would give OS firmware {chosen.version}; Sekka's drivers were tested "
                f"with {', '.join(self.profile['firmware']['os_firmware'])}.",
            )
        return chosen

    # Helpers for the stages ------------------------------------------------------

    def progress(self, stage, done, total):
        last = self.progress_state.get(stage, -PROGRESS_STEP)
        if done - last >= PROGRESS_STEP or done >= total:
            self.progress_state[stage] = done
            self.t.progress(stage, done, total)

    def refreshed(self):
        self.view.refresh()
        return self.view

    def open_working_view(self):
        """The view the stages work through. It mounts read-write only
        Sekka's own volumes (the planned stub's, once it exists, and the
        planned ESP) and everything else read-only, and unmounts all it
        mounted when the run ends."""
        from .asahi import Mounts, View

        self.working = View(self.asahi, Mounts(self.writable))
        self.view = self.working

    def close_working_view(self):
        if self.working is not None:
            try:
                self.working.close()
            except Exception as error:
                log.warning("couldn't unmount what the run mounted: %s", error)
            self.working = None

    def writable(self, info, dutil):
        if self.writable_esp and (info.get("DiskUUID") or "").lower() == self.writable_esp:
            return True
        ref = info.get("APFSContainerReference")
        container = dutil.ctnr_by_ref.get(ref) if ref else None
        return bool(
            self.writable_container
            and container
            and (container.get("APFSContainerUUID") or "").lower() == self.writable_container
            and not info.get("PartitionMapPartition")  # a volume, not the container's store
        )

    def workdir_bytes(self):
        """What the engine's own files occupy on macOS's volume now (the
        spool of Apple's files): removed when the run ends."""
        total = 0
        for dirpath, dirnames, filenames in os.walk(self.workdir):
            for name in filenames:
                st = os.lstat(os.path.join(dirpath, name))
                if stat.S_ISREG(st.st_mode):
                    total += st.st_blocks * 512
        return total

    def require_partition(self, gpt_uuid, what):
        p = self.view.partition(gpt_uuid)
        if p is None:
            raise StageFailure("partition-missing", f"{what} ({gpt_uuid}) is no longer on the disk.")
        return p

    def sync(self):
        self.host.run(["sync"], timeout=600)

    def remount(self, device):
        """Unmount and mount again, so what is read next comes from the disk
        rather than what the system cached while writing."""
        self.sync()
        self.view.dutil.unmount(device)
        return self.view.dutil.mount(device)

    def stub_os(self):
        p = self.require_partition(self.stub_uuid, "Sekka's startup volume")
        oses = [o for o in (p.os or []) if (o.vgid or "").lower() == self.stub["volume_group"]]
        if len(oses) != 1:
            raise StageFailure("stub-missing", "Sekka's startup volume no longer holds its system.")
        return p, oses[0]

    # The stages ------------------------------------------------------------------

    def stage_apple_inputs(self):
        a = self.asahi
        v = self.view
        self.ins = a.stub.StubInstaller(v.sysinfo, v.dutil, v.osinfo)
        done = [0]

        def progress(n, total):
            done[0] += n
            self.progress("apple-inputs", done[0], total)

        try:
            r = apple_inputs.prepare(a, self.ins, self.ipsw, self.workdir, progress=progress, room=self.apple_room)
        except apple_inputs.NoRoom as error:
            # A smaller Sekka leaves a shrunk macOS more room; macOS beside
            # free space has only what it has free.
            then = (
                "check this Mac again for a new plan"
                if self.shrink
                else "free some space in macOS, then check this Mac again"
            )
            raise StageFailure(
                "apple-inputs-no-room",
                f"Apple's startup files for the new system need {error.needed} bytes on macOS's volume while "
                f"Sekka installs, more than this plan leaves room for ({error.room} bytes). Nothing on the "
                f"disk changed; {then}.",
            )
        except Exception as error:
            log.exception("Apple's inputs")
            raise StageFailure(
                "apple-inputs-failed",
                "Apple's startup files for the new system couldn't be downloaded over HTTPS and checked "
                "against the archive's own CRC-32 checksums. Nothing on the disk changed; try again when "
                "the network is working.",
                str(error),
            )
        self.restore = r
        facts = [
            c.fact("os-firmware", self.ipsw.version),
            c.fact("restore-url", r["url"]),
            c.fact("restore-bytes", r["remote_size"]),
            c.fact("build", r["build"]),
            c.fact("build-manifest", r["manifest_digest"]),
            c.fact("members", r["members"]),
            c.fact("member-bytes", r["member_bytes"]),
            c.fact("fetched-bytes", r["fetched"]),
            c.fact(
                "authenticity",
                f"HTTPS to {r['host']} verified with the pinned runtime's CA bundle; URL pinned by "
                "Asahi v0.9.2; each member's zip CRC-32. No digest of Apple's bytes is checked.",
            ),
        ]
        return facts, "inspected"

    def stage_shrink(self):
        s = self.shrink
        v = self.refreshed()
        part = self.require_partition(self.shrink_partition, "The macOS container")
        if part.size != s["from"] or part.container is None:
            raise StageFailure("plan-mismatch", "The macOS container changed size before the shrink.")
        container = c.uuid_any(part.container["APFSContainerUUID"])
        if container != s["container"]:
            raise StageFailure("plan-mismatch", "Another container now occupies that partition.")
        doc, inspector = inspect_mac(self.asahi, self.host, self.profile, privileged=True)
        k = [x for x in doc["disk"]["containers"] if x["uuid"] == container][0]
        # The engine's own spool of Apple's files is counted as used now and
        # deleted when the run ends: it doesn't raise the minimum macOS keeps.
        spool = self.workdir_bytes()
        if k["resize"] is None or s["to"] < k["resize"]["minimum"] - spool:
            raise StageFailure(
                "shrink-too-far",
                "macOS now needs more room than when the plan was made; nothing was resized."
                + (
                    ""
                    if k["resize"] is None
                    else f" Its minimum is now {k['resize']['minimum']} bytes ({spool} of them the installer's "
                    f"own temporary files); the plan shrinks it to {s['to']}."
                ),
            )
        v = self.refreshed()
        part = self.require_partition(self.shrink_partition, "The macOS container")
        offset = part.offset
        try:
            # Asahi's DiskUtil, capturing diskutil's own words for the record.
            v.dutil.action("apfs", "resizeContainer", part.name, str(s["to"]))
        except subprocess.CalledProcessError as error:
            raise StageFailure(
                "resize-failed",
                "Apple's diskutil couldn't shrink the macOS container. Its message is below. The "
                "installer doesn't delete snapshots, turn off FileVault or retry the resize; "
                "check the disk with First Aid, then check this Mac again.",
                called_process_diagnostic(error),
            )
        v = self.refreshed()
        part = self.require_partition(self.shrink_partition, "The macOS container")
        if part.size != s["to"] or part.offset != offset or part.container is None:
            raise StageFailure(
                "resize-unexpected",
                f"diskutil reported success, but the container is now {part.size} bytes, not {s['to']}.",
            )
        if c.uuid_any(part.container["APFSContainerUUID"]) != container or part.container["CapacityCeiling"] != s["to"]:
            raise StageFailure("resize-unexpected", "The container's own size doesn't match its partition.")
        free = [p for p in v.parts if p.free and p.offset == part.offset + part.size]
        facts = [
            c.fact("container", container),
            c.fact("partition", c.uuid_any(part.uuid)),
            c.fact("size-before", s["from"]),
            c.fact("size-after", part.size),
            c.fact("free-offset", free[0].offset if free else 0),
            c.fact("free-size", free[0].size if free else 0),
        ]
        return facts, "inspected"

    def stage_stub_partition(self):
        plan = self.plan
        target = plan["target"]
        end = target["offset"] + target["size"]
        v = self.refreshed()
        if self.shrink:
            prev = self.require_partition(self.shrink_partition, "The macOS container")
            free = [p for p in v.parts if p.free and p.offset == prev.offset + prev.size]
        else:
            free = [p for p in v.parts if p.free and p.offset <= target["offset"] and p.offset + p.size >= end]
        if not free or free[0].offset > target["offset"] or free[0].offset + free[0].size < end:
            raise StageFailure("place-gone", "The free space for Sekka isn't where the plan puts it.")
        after = free[0].name
        before = {p.uuid for p in v.parts if not p.free}
        try:
            new = v.dutil.addPartition(after, "apfs", plan["volume_name"], str(c.STUB_BYTES))
        except subprocess.CalledProcessError as error:
            raise StageFailure(
                "partition-failed",
                "diskutil couldn't create Sekka's startup volume.",
                called_process_diagnostic(error),
            )
        v = self.refreshed()
        stub = v.partition(new.uuid) if new is not None and new.uuid not in before else None
        if stub is None:
            raise StageFailure("partition-unexpected", "diskutil reported success, but no new partition appeared.")
        ok = (
            stub.type == "Apple_APFS"
            and stub.container is not None
            and stub.size == c.STUB_BYTES
            and target["offset"] - c.ALIGN < stub.offset <= target["offset"]
            and stub.offset >= free[0].offset
            and stub.offset + stub.size <= end
        )
        if not ok:
            raise StageFailure(
                "partition-unexpected",
                f"The new startup volume is at {stub.offset} ({stub.size} bytes), not where the plan puts it.",
            )
        self.stub_uuid = c.uuid_any(stub.uuid)
        self.stub_container = c.uuid_any(stub.container["APFSContainerUUID"])
        self.writable_container = self.stub_container
        facts = [
            c.fact("stub-partition", self.stub_uuid),
            c.fact("stub-container", self.stub_container),
            c.fact("stub-offset", stub.offset),
            c.fact("stub-size", stub.size),
        ]
        return facts, "inspected"

    def stage_stub_system(self):
        v = self.refreshed()
        part = self.require_partition(self.stub_uuid, "Sekka's startup volume")
        cur_os = v.current_os()
        if cur_os is None:
            raise StageFailure("macos-missing", "The running macOS can't be found to copy its administrators.")
        ins = self.ins
        ins.sysinfo, ins.dutil, ins.osinfo = v.sysinfo, v.dutil, v.osinfo
        ins.prepare_volume(part)
        ins.check_volume()
        ins.install_files(cur_os)
        osi = ins.osi
        # Asahi's second step is replaced by Sekka's Recovery app.
        for path in (os.path.join(osi.system, "Finish Installation.app"),):
            if os.path.lexists(path):
                shutil.rmtree(path)
        for path in (ins.iapm_path, ins.iapm_dis_path):
            if os.path.lexists(path):
                os.unlink(path)
        admins = ins.stub_info.get("admin_users", {})
        if self.owner not in admins:
            raise StageFailure(
                "owner-missing",
                f"{self.owner} isn't among the administrators copied to the startup volume, so Recovery "
                "couldn't ask for their password.",
            )
        self.full_name = admins[self.owner].get("real_name")
        vgid = c.uuid_any(osi.vgid)
        v = self.refreshed()
        part = self.require_partition(self.stub_uuid, "Sekka's startup volume")
        oses = [o for o in (part.os or []) if (o.vgid or "").lower() == vgid]
        if len(oses) != 1 or not oses[0].stub or oses[0].version != ins.install_version:
            raise StageFailure("stub-unexpected", "The startup volume doesn't hold the stub system Asahi installs.")
        o = oses[0]
        volumes = {x["DeviceIdentifier"]: x for x in part.container.get("Volumes", [])}
        self.stub = {
            "container": self.stub_container,
            "volume_group": vgid,
            "system_volume": c.uuid_any(volumes[o.sys_volume]["APFSVolumeUUID"]),
            "data_volume": c.uuid_any(volumes[o.data_volume]["APFSVolumeUUID"]),
        }
        admin_plist = os.path.join(o.preboot, o.vgid, "var/db/AdminUserRecoveryInfo.plist")
        if not os.path.isfile(admin_plist):
            raise StageFailure("owner-missing", "The startup volume has no administrator records for Recovery.")
        facts = [
            c.fact("stub-volume-group", vgid),
            c.fact("stub-system-volume", self.stub["system_volume"]),
            c.fact("stub-data-volume", self.stub["data_volume"]),
            c.fact("stub-macos", o.version),
            c.fact("build", self.restore["build"]),
            c.fact("admin-users", len(admins)),
        ]
        return facts, "inspected"

    def stage_partitions(self):
        plan = self.plan
        a = plan["allocation"]
        end = plan["target"]["offset"] + plan["target"]["size"]
        v = self.refreshed()
        stub = self.require_partition(self.stub_uuid, "Sekka's startup volume")
        osins = self.asahi.osinstall.OSInstaller(v.dutil, self.installer_data, self.template)
        osins.name = plan["volume_name"]
        osins.pkg = self.payload
        osins.partition_disk(stub.name, a["esp"] + a["root"])
        self.osins = osins
        if len(osins.part_info) != 2:
            raise StageFailure("partition-unexpected", "Asahi's installer didn't create exactly two partitions.")
        v = self.refreshed()
        stub = self.require_partition(self.stub_uuid, "Sekka's startup volume")
        esp = self.require_partition(osins.part_info[0].uuid, "The new ESP")
        root = self.require_partition(osins.part_info[1].uuid, "The new root partition")
        stub_end = stub.offset + stub.size
        ok = (
            esp.type == "EFI"
            and esp.size == a["esp"]
            and stub_end <= esp.offset < stub_end + c.ALIGN
            and root.type == "Linux Filesystem"
            and root.size == a["root"]
            and esp.offset + esp.size <= root.offset < esp.offset + esp.size + c.ALIGN
            and root.offset + root.size <= end
        )
        if not ok:
            raise StageFailure(
                "partition-unexpected",
                "The ESP and root partitions aren't the sizes or in the places the plan says.",
            )
        self.esp_uuid = c.uuid_any(esp.uuid)
        self.root_uuid = c.uuid_any(root.uuid)
        self.writable_esp = self.esp_uuid
        facts = [
            c.fact("esp-partition", self.esp_uuid),
            c.fact("esp-offset", esp.offset),
            c.fact("esp-size", esp.size),
            c.fact("esp-volume-id", self.template["partitions"][0].get("volume_id", "none")),
            c.fact("root-partition", self.root_uuid),
            c.fact("root-offset", root.offset),
            c.fact("root-size", root.size),
        ]
        return facts, "inspected"

    def stage_firmware(self):
        fw = os.path.join(self.workdir, "vendorfw")
        if os.path.lexists(fw):
            shutil.rmtree(fw)
        os.makedirs(fw)
        pkg = self.asahi.asahi_firmware_core.FWPackage("vendorfw")
        self.ins.collect_firmware(pkg)
        pkg.close()
        source = {}
        for rel, path in sorted(walk_files(fw).items()):
            source[rel] = c.sha256_file(path)
        if "firmware.cpio" not in source:
            raise StageFailure("firmware-missing", "Asahi's firmware collection produced no firmware.cpio.")
        self.firmware_files = source
        v = self.refreshed()
        esp = self.require_partition(self.esp_uuid, "The new ESP")
        mount = v.dutil.mount(esp.name)
        dest = os.path.join(mount, "vendorfw")
        if os.path.lexists(dest):
            raise StageFailure("esp-not-empty", "The new ESP already has a vendorfw folder.")
        shutil.copytree(fw, dest)
        remove_appledouble_sidecars(dest)
        mount = self.remount(esp.name)
        found = {rel: c.sha256_file(path) for rel, path in walk_files(os.path.join(mount, "vendorfw")).items()}
        if found != source:
            raise StageFailure("firmware-read-back-mismatch", "The firmware read back from the ESP isn't what was written.")
        inventory = A.esp_inventory_digest(
            [{"path": "vendorfw/" + rel, "size": s, "digest": d} for rel, (s, d) in source.items()]
        )
        facts = [
            c.fact("firmware-files", len(source)),
            c.fact("firmware-cpio", source["firmware.cpio"][1]),
            c.fact("firmware-cpio-size", source["firmware.cpio"][0]),
            c.fact("firmware-inventory", inventory),
        ]
        return facts, "read-back"

    def stage_esp(self):
        plan = self.plan
        v = self.refreshed()
        esp = self.require_partition(self.esp_uuid, "The new ESP")
        mount = v.dutil.mount(esp.name)
        expected = {}
        # Zips needn't list a file's folders before it; Asahi's extract_tree
        # creates only the folders it meets, so make the parents first.
        # Adapted from Omarchy's OmarchyOSInstaller.extract_tree (MIT, see the
        # module docstring).
        for info in self.payload.infolist():
            if info.filename.startswith(A.ESP_PREFIX) and not info.filename.endswith("/"):
                rel = info.filename[len(A.ESP_PREFIX) :]
                os.makedirs(os.path.dirname(os.path.join(mount, rel)) or mount, exist_ok=True)
        self.osins.extract_tree("esp", mount)
        for rel, item in self.esp_files.items():
            expected[rel] = (item["size"], item["digest"])
        entry_rel = self.manifest["esp"]["boot_entry"]
        entry_path = os.path.join(mount, entry_rel)
        with open(entry_path, "rb") as f:
            entry = f.read().decode("utf-8")
        filled_entry = c.fill_entry(entry, self.root_uuid, self.esp_uuid)
        filled = filled_entry.encode("utf-8")
        with open(entry_path + ".new", "wb") as f:
            f.write(filled)
            f.flush()
            os.fsync(f.fileno())
        os.replace(entry_path + ".new", entry_path)
        expected[entry_rel] = (len(filled), canonical.digest_bytes(filled))
        hints = self.hints
        handoff = c.handoff(
            self.installation,
            plan["release"]["release"],
            plan["release"]["source_revision"],
            self.root_uuid,
            self.esp_uuid,
            c.format_timestamp(self.host.now()),
            hints,
        )
        handoff_bytes = canonical.dumps(handoff)
        os.makedirs(os.path.join(mount, os.path.dirname(c.HANDOFF_PATH)), exist_ok=True)
        with open(os.path.join(mount, c.HANDOFF_PATH), "wb") as f:
            f.write(handoff_bytes)
            f.flush()
            os.fsync(f.fileno())
        expected[c.HANDOFF_PATH] = (len(handoff_bytes), canonical.digest_bytes(handoff_bytes))
        for rel, value in self.firmware_files.items():
            expected["vendorfw/" + rel] = value
        if self.esp_template().get("copy_installer_data"):
            # Asahi's own installer data (stub_info.json, BuildManifest, the
            # firmware archive), which its firmware tools read from Linux.
            data = os.path.join(mount, "asahi")
            os.makedirs(data, exist_ok=True)
            self.ins.collect_installer_data(data)
            for src, name in self.ins.copy_idata:
                expected["asahi/" + name] = c.sha256_file(src)
            stub_info = json.dumps(self.ins.stub_info).encode("utf-8")
            expected["asahi/stub_info.json"] = (len(stub_info), canonical.digest_bytes(stub_info))
        remove_appledouble_sidecars(mount)
        mount = self.remount(esp.name)
        found = {}
        for rel, path in walk_files(mount, skip=MACOS_BOOKKEEPING).items():
            found[rel] = c.sha256_file(path)
        if found != expected:
            wrong = sorted(set(found) ^ set(expected) | {k for k in found if k in expected and found[k] != expected[k]})
            raise StageFailure(
                "esp-read-back-mismatch",
                f"The ESP read back differs from what was written: {', '.join(wrong[:5])}.",
            )
        # Verified: nothing more is written to the ESP, and a FAT volume left
        # mounted would be writable by any of the owner's processes.
        self.sync()
        self.view.dutil.unmount(esp.name)
        files = [{"path": rel, "size": s, "digest": d} for rel, (s, d) in found.items()]
        provided = [k for k in ("locale", "keyboard", "timezone", "computer_name", "full_name") if hints.get(k)]
        facts = [
            c.fact("esp-inventory", A.esp_inventory_digest(files)),
            c.fact("esp-files", len(files)),
            c.fact("esp-bytes", sum(f["size"] for f in files)),
            c.fact("boot-entry", expected[entry_rel][1]),
            # As the entry written and read back passes them.
            c.fact("kernel-arguments", c.partition_arguments(filled_entry)),
            c.fact("handoff", expected[c.HANDOFF_PATH][1]),
            c.fact("installation", self.installation),
            c.fact("setup-hints", ", ".join(provided) or "none"),
        ]
        return facts, "read-back"

    def esp_template(self):
        return self.template["partitions"][0]

    def stage_root(self):
        self.refreshed()
        root = self.require_partition(self.root_uuid, "The new root partition")
        image = self.manifest["root_image"]
        size = image["size"]
        info = self.payload.getinfo(A.ROOT_IMAGE)
        if info.file_size != size or size > root.size:
            raise StageFailure("root-too-large", "The root image doesn't fit its partition.")
        total = 2 * size
        source = hashlib.sha256()
        written = 0
        with self.host.open_raw(root.name, writable=True) as dev:
            pending = bytearray()
            with self.payload.open(info) as src:
                while True:
                    block = src.read(CHUNK)
                    if block:
                        source.update(block)
                        pending.extend(block)
                    while len(pending) >= CHUNK or (not block and pending):
                        n = min(CHUNK, len(pending))
                        view = memoryview(pending)[:n]
                        offset = 0
                        while offset < n:
                            offset += dev.write(view[offset:])
                        view.release()
                        del pending[:n]
                        written += n
                        self.progress("root", written, total)
                    if not block:
                        break
            flush = self.host.flush_device(dev)
        source_digest = "sha256:" + source.hexdigest()
        if written != size or source_digest != image["digest"]:
            raise StageFailure(
                "root-source-mismatch",
                "The root image read from the package isn't the manifest's; the partition holds it anyway.",
            )
        back = hashlib.sha256()
        read = 0
        with self.host.open_raw(root.name, writable=False) as dev:
            while read < size:
                block = dev.read(min(CHUNK, size - read))
                if not block:
                    break
                back.update(block)
                read += len(block)
                self.progress("root", size + read, total)
        back_digest = "sha256:" + back.hexdigest()
        if read != size or back_digest != image["digest"]:
            raise StageFailure(
                "root-read-back-mismatch",
                "The root partition, read back, doesn't hold the image that was written.",
                f"wrote {source_digest}, read back {back_digest} over {read} of {size} bytes",
            )
        facts = [
            c.fact("root-partition", self.root_uuid),
            c.fact("root-image-size", size),
            c.fact("root-image", source_digest),
            c.fact("root-read-back", back_digest),
            c.fact("flush", flush),
        ]
        return facts, "read-back"

    def stage_recovery(self):
        plan = self.plan
        v = self.refreshed()
        _, o = self.stub_os()
        system = o.system
        app_dest = os.path.join(system, RECOVERY_APP)
        data_dir = os.path.join(system, c.RECOVERY_DATA_DIR)
        if os.path.lexists(app_dest) or os.path.lexists(data_dir):
            raise StageFailure("recovery-not-empty", "The startup volume already has a Recovery app or its data.")
        staging = os.path.join(system, f".sekka-recovery-{self.installation}")
        if os.path.lexists(staging):
            shutil.rmtree(staging)
        os.mkdir(staging, 0o755)
        for info, kind, mode, size, digest in self.recovery_members:
            parts = A.check_relative(info.filename, allow_dir=(kind == "dir"))
            target = os.path.join(staging, *parts)
            if kind == "dir":
                os.makedirs(target, mode=0o755, exist_ok=True)
                continue
            os.makedirs(os.path.dirname(target), mode=0o755, exist_ok=True)
            fd = os.open(target, os.O_WRONLY | os.O_CREAT | os.O_EXCL | getattr(os, "O_NOFOLLOW", 0), mode)
            h = hashlib.sha256()
            with os.fdopen(fd, "wb") as out, self.recovery_zip.open(info) as src:
                while True:
                    block = src.read(CHUNK)
                    if not block:
                        break
                    h.update(block)
                    out.write(block)
                out.flush()
                os.fsync(out.fileno())
            os.chmod(target, mode)
            if "sha256:" + h.hexdigest() != digest:
                raise StageFailure("recovery-app-changed", f"{info.filename} changed while it was copied.")
        os.rename(os.path.join(staging, RECOVERY_APP), app_dest)
        os.rmdir(staging)
        os.makedirs(data_dir, mode=0o755)
        boot = os.path.join(data_dir, "boot.bin")
        next_object = self.template["next_object"]
        # As osinstall.install builds it: Asahi's m1n1 stage 1 with the ESP
        # named and the new ESP's m1n1/boot.bin to chainload.
        variables = [
            f"chosen.asahi,efi-system-partition={self.esp_uuid}",
            f"chainload={self.esp_uuid};{next_object}",
        ]
        self.asahi.m1n1.build(os.path.join(self.asahi.root, "boot", self.template["boot_object"]), boot, variables)
        boot_size, boot_digest = c.sha256_file(boot)
        firmware = []
        for rel, (size, digest) in sorted(self.firmware_files.items()):
            dest = os.path.join(data_dir, "firmware", rel)
            os.makedirs(os.path.dirname(dest), exist_ok=True)
            shutil.copyfile(os.path.join(self.workdir, "vendorfw", rel), dest)
            firmware.append(c.inventory_file("firmware/" + rel, size, digest))
        config = {
            "schema": c.RECOVERY_SCHEMA,
            "installation": self.installation,
            "release": plan["release"]["release"],
            "volume_name": plan["volume_name"],
            "stub": dict(self.stub),
            "esp": self.esp_uuid,
            "root": self.root_uuid,
            "boot_object": c.inventory_file("boot.bin", boot_size, boot_digest),
            "firmware": firmware,
            "owner": {
                "short_name": c.text(self.owner),
                "full_name": c.text(c.plain_text(self.full_name or "", limit=255, keep="start")),
                "macos_volume_group": plan["disk"]["macos_volume_group"],
            },
            "os_firmware": c.version(self.ipsw.version),
        }
        c.check_recovery_config(config)
        config_bytes = canonical.dumps(config)
        if len(config_bytes) > c.LIMIT_RECOVERY:
            raise StageFailure("recovery-config-too-large", "The Recovery configuration is too large.")
        with open(os.path.join(data_dir, c.RECOVERY_CONFIG_FILE), "wb") as f:
            f.write(config_bytes)
            f.flush()
            os.fsync(f.fileno())
        with open(os.path.join(self.asahi.root, "step2", "IAPhysicalMedia.plist"), "rb") as f:
            media = plistlib.load(f)
        media["AppName"] = RECOVERY_APP
        media_bytes = plistlib.dumps(media)
        iapm = os.path.join(system, ".IAPhysicalMedia")
        with open(iapm, "wb") as f:
            f.write(media_bytes)
            f.flush()
            os.fsync(f.fileno())
        # Read everything back from the disk.
        self.sync()
        v.dutil.unmount(o.sys_volume)
        v = self.refreshed()
        _, o = self.stub_os()
        system = v.dutil.mount(o.sys_volume)
        app_inventory, app_files = A.recovery_inventory(self.recovery_members)
        found = walk_files(os.path.join(system, RECOVERY_APP))
        want = {f["path"].split("/", 1)[1]: (f["size"], f["digest"]) for f in app_files}
        if {rel: c.sha256_file(p) for rel, p in found.items()} != want:
            raise StageFailure("recovery-read-back-mismatch", "The Recovery app read back isn't the one written.")
        checks = {
            os.path.join(c.RECOVERY_DATA_DIR, c.RECOVERY_CONFIG_FILE): (len(config_bytes), canonical.digest_bytes(config_bytes)),
            os.path.join(c.RECOVERY_DATA_DIR, "boot.bin"): (boot_size, boot_digest),
            ".IAPhysicalMedia": (len(media_bytes), canonical.digest_bytes(media_bytes)),
        }
        for item in firmware:
            checks[os.path.join(c.RECOVERY_DATA_DIR, item["path"])] = (item["size"], item["digest"])
        for rel, value in checks.items():
            if c.sha256_file(os.path.join(system, rel)) != value:
                raise StageFailure("recovery-read-back-mismatch", f"{rel} read back isn't what was written.")
        facts = [
            c.fact("recovery-app", app_inventory),
            c.fact("recovery-app-files", len(app_files)),
            c.fact("recovery-config", canonical.digest_bytes(config_bytes)),
            c.fact("boot-object", boot_digest),
            c.fact("boot-object-size", boot_size),
            c.fact(
                "firmware-baseline",
                A.esp_inventory_digest([{"path": f["path"], "size": f["size"], "digest": f["digest"]} for f in firmware]),
            ),
            c.fact("iaphysicalmedia", canonical.digest_bytes(media_bytes)),
        ]
        return facts, "read-back"

    def stage_startup_disk(self):
        v = self.refreshed()
        _, o = self.stub_os()
        ins = self.ins
        ins.osi = o
        ins.get_paths()
        ins.prepare_for_bless()
        password = self.password
        try:
            # Asahi's bless(): the owner's password on standard input.
            result = self.host.run(
                ["bless", "--setBoot", "--device", "/dev/" + o.sys_volume, "--user", self.owner, "--stdinpass"],
                input=bytes(password),
                timeout=600,
            )
        finally:
            wipe(password)
            self.password = None
        if result.returncode != 0:
            raise StageFailure(
                "startup-disk-failed",
                "macOS didn't make Sekka the startup disk. Everything else is in place and verified; the "
                "startup disk can be set again without changing anything else.",
                f"{result.output.decode('utf-8', 'replace').strip()}\n(bless exited with status {result.returncode})",
            )
        # bless's exit status isn't enough: read the selection back two ways.
        # (After Omarchy's verify_default_boot; MIT, see the module docstring.)
        group = self.stub["volume_group"]
        got = self.host.run(["bless", "--getBoot"], timeout=60)
        device = (got.stdout or b"").decode("utf-8", "replace").strip()
        selected = None
        if got.returncode == 0 and device.startswith("/dev/disk"):
            try:
                info = v.dutil.get("info", "-plist", device)
                selected = (info.get("APFSVolumeGroupID") or "").lower() or None
            except subprocess.CalledProcessError:
                selected = None
        v.sysinfo.get_nvram_data()
        nvram = (v.sysinfo.default_boot or "").lower() or None
        if selected != group or nvram != group:
            raise StageFailure(
                "startup-disk-not-selected",
                "bless reported success, but the startup disk isn't Sekka's. The password may have been "
                "refused; the startup disk can be set again without changing anything else.",
                f"bless --getBoot: {device or '(nothing)'} (group {selected}); nvram boot-volume group {nvram}",
            )
        # As Asahi does after bless: hide the stub's SystemVersion so that
        # starting it opens Recovery, where .IAPhysicalMedia names the app.
        ins.prepare_for_step2()
        if os.path.exists(ins.sv_path) or not os.path.exists(ins.iapm_path):
            raise StageFailure("stub-unexpected", "The startup volume isn't ready for Recovery.")
        facts = [
            c.fact("startup-volume-group", group),
            c.fact("bless-boot-device", device),
            c.fact("nvram-boot-volume-group", nvram),
            c.fact("recovery-app", RECOVERY_APP),
        ]
        return facts, "policy-read-back"
