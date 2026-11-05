"""Training scenarios: faults and tasks for making fine-tuning data, kept
apart from the evaluation's (scenarios.py, the held-out set).

They use the same machine (machine.nix), base class, checks and
invariants, and are validated the same way (`run.py --set train --agent
none`: the check fails; `--agent oracle`: it passes with nothing else
harmed). Each has a `topic`, for balancing the data.

None is a paraphrase of a held-out scenario or the same fault on another
unit or file. Each training scenario beside the nearest held-out one:

| Training scenario | Area | Nearest held-out | How it differs |
|---|---|---|---|
| root-full-coredumps | sekka | disk-full | the root filesystem, filled by crash dumps; saving files fails, not the backup on /srv/data |
| journal-not-stored | sekka | (none) | a journald drop-in with `Storage=none`; nothing held-out touches journald |
| wired-connection-off | sekka | dns-nameserver | the NetworkManager profile is down with autoconnect off; no resolver change |
| nsswitch-no-dns | sekka | dns-nameserver | resolv.conf and the server are right; nsswitch.conf's hosts line lacks `dns` |
| groups-replaced | sekka | (none) | `usermod -G` without `-a` dropped alice from wheel and networkmanager |
| tmp-not-world-writable | sekka | ssh-permissions | a system directory's mode (/tmp lost 1777), not ~/.ssh or ownership |
| printer-paused | sekka | (none) | a CUPS queue disabled and rejecting jobs |
| immutable-budget | sekka | notes-owned-by-root | an ext4 immutable attribute on one file, not ownership or mode |
| curl-proxy-leftover | sekka | hosts-override | a proxy in alice's ~/.curlrc; root's curl works; no hosts, DNS or firewall change |
| old-router-route | sekka | firewall-killswitch | a more specific static route via a dead gateway, not an nftables drop |
| notes-snapshot-timer-off | sekka | runaway-indexer | a user timer that should run is disabled; the held-out unit must be stopped |
| photo-import-tilde | sekka | backup-env-typo | a user service's EnvironmentFile uses `~`, which systemd doesn't expand; not a misspelt path |
| dangling-command-link | sekka | (none) | a symlink in ~/.local/bin whose target moved |
| share-port-closed | sekka | firewall-killswitch | inbound: the NixOS firewall rightly lacks a port alice needs open; not an outbound drop table |
| bashrc-path-overwritten | sekka | (none) | ~/.bashrc replaces PATH instead of extending it |
| pdf-opens-in-text-editor | aizome | (none) | ~/.config/mimeapps.list sends PDFs to the Text Editor |
| calculator-hidden | aizome | (none) | a user desktop entry with `Hidden=true` hides Soroban |
| session-locale-c | aizome | keyboard-layout | `LC_ALL=C` in ~/.config/aizome/environment, the session's environment, not settings.toml |
| speakers-muted | aizome | pipewire-masked | PipeWire runs and lists the outputs; the default one is muted at volume 0 |
| screenshots-folder-link | aizome | (none) | ~/Pictures/Screenshots is a dangling link to a USB stick |
| alarm-not-scheduled | aizome | (none) | a Clock alarm whose transient user timer was never scheduled |
| terminal-shell-missing | aizome | settings-syntax | another app's file (hotaru.toml) parses, but names a shell that isn't installed |
| ssh-agent-stopped | aizome | ssh-permissions | the session's key agent (gcr-ssh-agent) is stopped; sshd and ~/.ssh are untouched |
| desktop-folder-renamed | aizome | (none) | user-dirs.dirs points the desktop at another folder |
| cu-terminal-font-size | aizome | cu-terminal-file | the terminal's own preferences, not a command in it |
| cu-alarm | aizome | (none) | the Clock app |
| cu-trash-readme | aizome | cu-folder | Files, moving a file to the Trash, not making a folder |
| cu-volume-half | aizome | cu-sound-output | the output's volume, not which output |
| cu-editor-save | aizome | cu-terminal-file | the Text Editor and its save dialog, not a terminal |
| cu-calendar-event | aizome | (none) | the Calendar app |
| cu-disk-usage | aizome | cu-calculator | another app (Disk Usage) to read an answer from |
| cu-pin-note | aizome | cu-note | pinning an existing note, not writing one |

Computer-use tasks have shell oracles here too: they reach the state the
app would leave (through the app's own command line where it has one), to
show the check can pass; that the app's controls do it is shown by the
model runs.
"""

import json
import re
import shlex
from dataclasses import dataclass

from scenarios import (  # noqa: F401 (collateral and diagnosed are this module's interface too)
    HOME,
    Scenario,
    alice_write,
    answer_of,
    collateral,
    diagnosed,
    lan_reachable,
    toml_value,
)


@dataclass
class Train(Scenario):
    # What the scenario exercises, for balancing the data.
    topic: str = ""


def root_write(vm, path, text, mode="0644"):
    vm.sh(f"mkdir -p $(dirname {shlex.quote(path)}) && cat > {shlex.quote(path)} <<'EOF'\n{text}EOF\n"
          f"chmod {mode} {shlex.quote(path)}")


def alice_value(vm, command):
    return vm.alice(command)[1].strip()


def watch_app(vm, name, binary):
    """Note whether an app ran, for a check made after it may have closed."""
    vm.sh(f"systemd-run --unit=eval-watch-{name} --property=Type=simple --setenv=PATH=/run/current-system/sw/bin "
          f"bash -c 'while sleep 0.5; do pgrep -u alice -f {binary} && touch /run/eval-{name}-ran; done' >/dev/null")


def app_ran(vm, name):
    return vm.ok(f"test -e /run/eval-{name}-ran")


# Diagnostics: the system.


class CoredumpsFillRoot(Train):
    DIR = "/var/lib/systemd/coredump"

    def setup(self, vm):
        # An app that crashed in a loop yesterday, its dumps filling the
        # root filesystem (as far as alice may write: root keeps ext4's
        # reserve), and the journal's record of the crashes.
        script = r"""
d=/var/lib/systemd/coredump; mkdir -p $d
boot=$(tr -d - < /proc/sys/kernel/random/boot_id)
avail=$(df --output=avail -B1 / | tail -1)
each=$(( (avail - 1048576) / 6 ))
now=$(date +%s)
for i in 1 2 3 4 5 6; do
  pid=$((2100 + i * 37)); t=$((now - 86400 + i * 600))
  f="$d/core.aizome-tokei.1000.$boot.$pid.${t}000000.zst"
  fallocate -l $each "$f"; chmod 0640 "$f"
  printf 'MESSAGE_ID=fc2e22bc6ee647b6b90729ab34a250b1\nMESSAGE=Process %s (aizome-tokei) of user 1000 dumped core.\nCOREDUMP_PID=%s\nCOREDUMP_UID=1000\nCOREDUMP_COMM=aizome-tokei\nCOREDUMP_EXE=/run/current-system/sw/bin/aizome-tokei\nCOREDUMP_SIGNAL=6\nCOREDUMP_FILENAME=%s\nPRIORITY=2\nSYSLOG_IDENTIFIER=systemd-coredump\n' "$pid" "$pid" "$f" > /run/eval-core.txt
  logger --journald=/run/eval-core.txt
done
rm -f /run/eval-core.txt; sync
"""
        vm.sh(script, timeout=120)

    def check(self, vm, final):
        avail = int(vm.out_of("df --output=avail -B1 / | tail -1").strip() or 0)
        writes = vm.alice("head -c 100M /dev/zero > ~/.eval-space-probe && rm -f ~/.eval-space-probe")[0] == 0
        vm.sh(f"rm -f {HOME}/.eval-space-probe")
        return avail >= 1 << 30 and writes, {"avail_mib": avail >> 20, "alice-can-write": writes}

    def oracle(self, vm):
        vm.sh(f"rm -f {self.DIR}/core.*")


JOURNAL_DROPIN = "/etc/systemd/journald.conf.d/50-save-ssd.conf"


class JournalNotStored(Train):
    def setup(self, vm):
        root_write(vm, JOURNAL_DROPIN, "# Fewer writes to the SSD.\n[Journal]\nStorage=none\n")
        vm.sh("systemctl restart systemd-journald; sleep 1")

    def check(self, vm, final):
        ok = vm.ok("tag=eval-probe-$RANDOM$RANDOM; logger -t evalprobe \"$tag\"; sleep 2; "
                   "journalctl --no-pager -t evalprobe --since -3min | grep -q \"$tag\"")
        return ok, {"dropins": vm.out_of("ls /etc/systemd/journald.conf.d 2>&1; "
                                         "cat /etc/systemd/journald.conf.d/*.conf 2>/dev/null")[-400:]}

    def oracle(self, vm):
        vm.sh(f"rm -f {JOURNAL_DROPIN} && systemctl restart systemd-journald")


WIRED = "Wired connection 1"


class WiredConnectionOff(Train):
    def setup(self, vm):
        vm.sh(f"nmcli connection modify '{WIRED}' connection.autoconnect no && nmcli connection down '{WIRED}'; sleep 3")

    def check(self, vm, final):
        state = vm.out_of("nmcli -g GENERAL.STATE device show eth0").strip()
        name = vm.out_of("nmcli -g GENERAL.CONNECTION device show eth0").strip()
        auto = vm.out_of(f"nmcli -g connection.autoconnect connection show {shlex.quote(name)}").strip() if name else ""
        route = vm.ok("ip route show default | grep -q 'dev eth0'")
        ok = state.startswith("100") and auto == "yes" and route
        return ok, {"state": state, "connection": name, "autoconnect": auto, "default-route": route}

    def oracle(self, vm):
        vm.sh(f"nmcli connection modify '{WIRED}' connection.autoconnect yes && nmcli connection up '{WIRED}'; sleep 2")


class NsswitchNoDns(Train):
    def setup(self, vm):
        vm.sh("cp --remove-destination \"$(readlink -f /etc/nsswitch.conf)\" /etc/nsswitch.conf && "
              "sed -i 's/^\\(hosts:.*\\) dns$/\\1/' /etc/nsswitch.conf && systemctl restart nscd")

    def check(self, vm, final):
        return lan_reachable(vm), {"hosts": vm.out_of("grep '^hosts:' /etc/nsswitch.conf").strip()}

    def oracle(self, vm):
        vm.sh("sed -i 's/^\\(hosts:.*myhostname\\)$/\\1 dns/' /etc/nsswitch.conf && systemctl restart nscd")


GROUPS_HISTORY = """ls ~/Pictures
sudo usermod -G video alice
groups
ls -l /dev/dri
sudo ls /root
"""


class GroupsReplaced(Train):
    def setup(self, vm):
        alice_write(vm, HOME + "/.bash_history", GROUPS_HISTORY)
        vm.sh("usermod -G video alice")

    def check(self, vm, final):
        groups = vm.out_of("id -nG alice").split()
        return all(g in groups for g in ("wheel", "networkmanager", "video")), {"groups": groups}

    def oracle(self, vm):
        vm.sh("usermod -aG wheel,networkmanager alice")


class TmpNotWorldWritable(Train):
    def setup(self, vm):
        vm.sh("chmod 0755 /tmp")

    def check(self, vm, final):
        mode = vm.out_of("stat -c '%a %U' /tmp").strip()
        writes = vm.alice("f=$(mktemp) && rm -f \"$f\"")[0] == 0
        return mode == "1777 root" and writes, {"mode": mode, "alice-mktemp": writes}

    def oracle(self, vm):
        vm.sh("chmod 1777 /tmp")


PRINTER_LISTENER = """import socket
s = socket.socket()
s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
s.bind(("10.88.0.2", 9100))
s.listen(4)
while True:
    c, _ = s.accept()
    while c.recv(65536):
        pass
    c.close()
"""


class PrinterPaused(Train):
    def setup(self, vm):
        # The office printer, on the LAN (a raw socket that takes jobs).
        root_write(vm, "/run/lan-printer.py", PRINTER_LISTENER)
        vm.sh("systemd-run --unit=lan-printer --description='The evaluation LAN: the office printer' "
              "--property=Type=simple /run/current-system/sw/bin/ip netns exec lan "
              "/run/current-system/sw/bin/python3 /run/lan-printer.py >/dev/null; sleep 1; "
              "lpadmin -p Office -D 'Office printer' -L Study -E -v socket://10.88.0.2:9100 -m raw "
              "-o printer-error-policy=retry-job 2>/dev/null && lpadmin -d Office && "
              "cupsdisable -r 'Paper jam' Office")
        vm.alice("lp -d Office ~/Documents/report.txt >/dev/null")
        vm.sh("cupsreject -r 'Paper jam' Office")

    def check(self, vm, final):
        printer = vm.out_of("lpstat -p Office 2>&1")
        accepting = vm.out_of("lpstat -a Office 2>&1")
        ok = "disabled" not in printer and "Office" in printer and "not accepting" not in accepting \
            and "accepting requests" in accepting
        return ok, {"printer": printer.strip()[:300], "accepting": accepting.strip()[:200]}

    def oracle(self, vm):
        vm.sh("cupsenable Office && cupsaccept Office")


BUDGET = """November budget
rent: 1200
groceries: 400
bus pass: 75
"""


class ImmutableBudget(Train):
    PATH = HOME + "/Documents/budget.txt"

    def setup(self, vm):
        alice_write(vm, self.PATH, BUDGET)
        vm.sh(f"chattr +i {self.PATH}")

    def check(self, vm, final):
        attrs = (vm.out_of(f"lsattr {self.PATH} 2>/dev/null").split() or ["missing"])[0]
        kept = vm.ok(f"grep -q 'rent: 1200' {self.PATH}")
        writable = vm.alice(f"test -w {self.PATH}")[0] == 0
        return "i" not in attrs and kept and writable, {"attrs": attrs, "kept": kept, "writable": writable}

    def oracle(self, vm):
        vm.sh(f"chattr -i {self.PATH}")


CURLRC = """proxy = "http://10.88.0.9:3128"
connect-timeout = 20
"""


class CurlProxyLeftover(Train):
    def setup(self, vm):
        alice_write(vm, HOME + "/.curlrc", CURLRC)

    def check(self, vm, final):
        ok = vm.alice("curl -sf --max-time 10 http://updates.sekka.test/ | grep -q 'server: ok'", timeout=30)[0] == 0
        return ok, {"curlrc": vm.out_of(f"cat {HOME}/.curlrc 2>&1")[-300:]}

    def oracle(self, vm):
        vm.alice("sed -i '/^proxy/d' ~/.curlrc")


class OldRouterRoute(Train):
    def setup(self, vm):
        vm.sh("ip route add 10.88.0.0/25 via 10.88.0.254 dev lan0")

    def check(self, vm, final):
        route = vm.out_of("ip route get 10.88.0.2").strip()
        return lan_reachable(vm) and "via" not in route, {"route": route}

    def oracle(self, vm):
        vm.sh("ip route del 10.88.0.0/25 via 10.88.0.254 dev lan0")


SNAPSHOT_SCRIPT = """#!/bin/sh
# Copies ~/Notes into Documents/notes-snapshots, one folder per run.
set -eu
d="$HOME/Documents/notes-snapshots/$(date +%Y%m%d-%H%M)"
mkdir -p "$d"
cp -a "$HOME/Notes/." "$d/"
"""

SNAPSHOT_SERVICE = """[Unit]
Description=Snapshot my notes

[Service]
Type=oneshot
ExecStart=%h/.local/bin/notes-snapshot
"""

SNAPSHOT_TIMER = """[Unit]
Description=Snapshot my notes every hour

[Timer]
OnCalendar=hourly
Persistent=true

[Install]
WantedBy=timers.target
"""


class NotesSnapshotTimerOff(Train):
    def setup(self, vm):
        alice_write(vm, HOME + "/.local/bin/notes-snapshot", SNAPSHOT_SCRIPT)
        alice_write(vm, HOME + "/.config/systemd/user/notes-snapshot.service", SNAPSHOT_SERVICE)
        alice_write(vm, HOME + "/.config/systemd/user/notes-snapshot.timer", SNAPSHOT_TIMER)
        vm.alice("chmod +x ~/.local/bin/notes-snapshot && d=~/Documents/notes-snapshots/$(date -d yesterday +%Y%m%d)-1600 && "
                 "mkdir -p $d && cp -a ~/Notes/. $d/ && touch -d 'yesterday 16:00' $d $d/* && "
                 "systemctl --user daemon-reload && systemctl --user disable --now notes-snapshot.timer 2>/dev/null; true")

    def check(self, vm, final):
        enabled = alice_value(vm, "systemctl --user is-enabled notes-snapshot.timer 2>&1")
        active = alice_value(vm, "systemctl --user show -p ActiveState --value notes-snapshot.timer")
        return enabled == "enabled" and active == "active", {"enabled": enabled, "active": active}

    def oracle(self, vm):
        vm.alice("systemctl --user enable --now notes-snapshot.timer")


PHOTO_SCRIPT = """#!/bin/sh
# Copies new photos from the camera folder into the photo library.
set -eu
: "${PHOTO_SOURCE:?PHOTO_SOURCE is not set}" "${PHOTO_LIBRARY:?PHOTO_LIBRARY is not set}"
if [ ! -d "$PHOTO_SOURCE" ]; then
  echo "photo source $PHOTO_SOURCE does not exist" >&2
  exit 1
fi
mkdir -p "$PHOTO_LIBRARY"
cp --update=none "$PHOTO_SOURCE"/* "$PHOTO_LIBRARY"/
echo "imported into $PHOTO_LIBRARY"
"""

PHOTO_ENV = """# Where photo-import copies from and to.
PHOTO_SOURCE=~/Pictures/Camera
PHOTO_LIBRARY=/home/alice/Pictures/Library
"""

PHOTO_SERVICE = """[Unit]
Description=Import photos from the camera folder

[Service]
Type=oneshot
EnvironmentFile=%h/.config/photo-import.env
ExecStart=%h/.local/bin/photo-import
"""


class PhotoImportTilde(Train):
    def setup(self, vm):
        alice_write(vm, HOME + "/.local/bin/photo-import", PHOTO_SCRIPT)
        alice_write(vm, HOME + "/.config/photo-import.env", PHOTO_ENV)
        alice_write(vm, HOME + "/.config/systemd/user/photo-import.service", PHOTO_SERVICE)
        vm.alice("chmod +x ~/.local/bin/photo-import && mkdir -p ~/Pictures/Camera && "
                 "head -c 40000 /dev/urandom > ~/Pictures/Camera/IMG_0412.jpg && "
                 "head -c 52000 /dev/urandom > ~/Pictures/Camera/IMG_0413.jpg && "
                 "systemctl --user daemon-reload && systemctl --user start photo-import.service; true")

    def check(self, vm, final):
        status, out = vm.alice("systemctl --user reset-failed photo-import.service 2>/dev/null; "
                               "systemctl --user start photo-import.service", timeout=60)
        copied = vm.ok(f"test -s {HOME}/Pictures/Library/IMG_0412.jpg && test -s {HOME}/Pictures/Library/IMG_0413.jpg")
        return status == 0 and copied, {"start": status, "output": out[-300:], "copied": copied,
                                        "env": vm.out_of(f"cat {HOME}/.config/photo-import.env")[-300:]}

    def oracle(self, vm):
        vm.alice("sed -i 's|^PHOTO_SOURCE=~|PHOTO_SOURCE=/home/alice|' ~/.config/photo-import.env")


EXPORT_SCRIPT = """#!/bin/sh
# Collects every note into one Markdown file.
set -eu
out="$HOME/Documents/all-notes.md"
: > "$out"
for f in "$HOME"/Notes/*.md; do
  printf '\\n<!-- %s -->\\n' "$(basename "$f")" >> "$out"
  cat "$f" >> "$out"
done
echo "wrote $out"
"""


class DanglingCommandLink(Train):
    def setup(self, vm):
        alice_write(vm, HOME + "/Documents/scripts/notes-export", EXPORT_SCRIPT)
        vm.alice("chmod +x ~/Documents/scripts/notes-export && mkdir -p ~/.local/bin && "
                 "ln -sfn /home/alice/scripts/notes-export ~/.local/bin/notes-export")

    def check(self, vm, final):
        vm.sh(f"rm -f {HOME}/Documents/all-notes.md")
        status, out = vm.alice("~/.local/bin/notes-export", timeout=30)
        wrote = vm.ok(f"grep -q coffee {HOME}/Documents/all-notes.md")
        return status == 0 and wrote, {"status": status, "output": out[-200:],
                                       "link": vm.out_of(f"ls -l {HOME}/.local/bin/notes-export 2>&1")[-200:]}

    def oracle(self, vm):
        vm.alice("ln -sfn /home/alice/Documents/scripts/notes-export ~/.local/bin/notes-export")


SHARE_SERVICE = """[Unit]
Description=Share ~/Public on the local network

[Service]
ExecStart=/run/current-system/sw/bin/python3 -m http.server 8000 --directory %h/Public
Restart=on-failure

[Install]
WantedBy=default.target
"""


class SharePortClosed(Train):
    def setup(self, vm):
        alice_write(vm, HOME + "/Public/shopping-list.txt", "eggs\nmilk\n")
        alice_write(vm, HOME + "/.config/systemd/user/share-folder.service", SHARE_SERVICE)
        vm.alice("systemctl --user daemon-reload && systemctl --user enable --now share-folder.service")
        vm.sh("sleep 2")

    def check(self, vm, final):
        ok = vm.ok("ip netns exec lan curl -sf --max-time 5 http://10.88.0.1:8000/ | grep -q shopping-list", timeout=30)
        return ok, {"service": alice_value(vm, "systemctl --user show -p ActiveState --value share-folder.service")}

    def oracle(self, vm):
        vm.sh("nixos-firewall-tool open tcp 8000")


BASHRC = """# ~/.bashrc
alias ll='ls -l'
export EDITOR=nano
# My own scripts.
export PATH="$HOME/bin"
"""


class BashrcPathOverwritten(Train):
    def setup(self, vm):
        alice_write(vm, HOME + "/.bashrc", BASHRC)
        alice_write(vm, HOME + "/bin/greet", "#!/bin/sh\necho \"hello, $USER\"\n")
        vm.alice("chmod +x ~/bin/greet")

    def check(self, vm, final):
        status, out = vm.alice("bash --norc --noprofile -c 'source ~/.bashrc >/dev/null 2>&1 </dev/null; "
                               "command -v ls && command -v nano && command -v greet && echo \"PATH=$PATH\"'",
                               timeout=30)
        return status == 0, {"output": out[-400:]}

    def oracle(self, vm):
        vm.alice("sed -i 's|^export PATH=\"$HOME/bin\"$|export PATH=\"$HOME/bin:$PATH\"|' ~/.bashrc")


# Diagnostics: the desktop session.


MIMEAPPS = """[Default Applications]
text/plain=org.aizome.Sumi.desktop
text/markdown=org.aizome.Sumi.desktop
application/pdf=org.aizome.Sumi.desktop

[Added Associations]
application/pdf=org.aizome.Sumi.desktop;org.aizome.Shiori.desktop;
"""

# The default for a type as the spec reads it: the user's desktop-specific
# list first, then the user's list; none there leaves the system's.
PDF_DEFAULT = r"""
import configparser, os, sys
home = "/home/alice/.config"
for name in ("aizome-mimeapps.list", "mimeapps.list"):
    p = os.path.join(home, name)
    if not os.path.exists(p):
        continue
    c = configparser.ConfigParser(interpolation=None, strict=False)
    c.optionxform = str
    try:
        c.read(p)
    except configparser.Error as e:
        print("unparsable:", e); sys.exit(0)
    v = c.get("Default Applications", "application/pdf", fallback=None)
    if v:
        print([x for x in v.split(";") if x][0]); sys.exit(0)
print("system")
"""


class PdfOpensInTextEditor(Train):
    def setup(self, vm):
        alice_write(vm, HOME + "/.config/mimeapps.list", MIMEAPPS)

    def check(self, vm, final):
        chosen = vm.out_of(f"python3 -c {shlex.quote(PDF_DEFAULT)}").strip().splitlines()[-1:]
        chosen = chosen[0] if chosen else ""
        xdg = alice_value(vm, "xdg-mime query default application/pdf")
        return chosen in ("org.aizome.Shiori.desktop", "system"), {"default": chosen, "xdg-mime": xdg}

    def oracle(self, vm):
        vm.alice("sed -i 's|^application/pdf=org.aizome.Sumi.desktop$|application/pdf=org.aizome.Shiori.desktop|' "
                 "~/.config/mimeapps.list")


SOROBAN_USER = HOME + "/.local/share/applications/org.aizome.Soroban.desktop"


class CalculatorHidden(Train):
    def setup(self, vm):
        vm.alice("mkdir -p ~/.local/share/applications && "
                 "cp /run/current-system/sw/share/applications/org.aizome.Soroban.desktop ~/.local/share/applications/ && "
                 "chmod u+w ~/.local/share/applications/org.aizome.Soroban.desktop && "
                 "sed -i '0,/^\\[Desktop Entry\\]/s//[Desktop Entry]\\nHidden=true/' "
                 "~/.local/share/applications/org.aizome.Soroban.desktop")

    def check(self, vm, final):
        hidden = vm.ok(f"test -e {SOROBAN_USER} && grep -Eiq '^(Hidden|NoDisplay)[[:space:]]*=[[:space:]]*true' {SOROBAN_USER}")
        system = vm.ok("test -e /run/current-system/sw/share/applications/org.aizome.Soroban.desktop")
        return not hidden and system, {"user-entry": vm.out_of(f"head -5 {SOROBAN_USER} 2>&1")[-300:]}

    def oracle(self, vm):
        vm.sh(f"rm -f {SOROBAN_USER}")


AIZOME_ENV = HOME + "/.config/aizome/environment"


class SessionLocaleC(Train):
    def setup(self, vm):
        alice_write(vm, AIZOME_ENV, "EDITOR=nano\nLC_ALL=C\n")
        vm.alice("printf 'coffee 3.50\\ncroissant 2.80\\n' > ~/Documents/$'Caf\\xc3\\xa9 receipts.txt'")
        # As the session applied it at login.
        vm.alice("systemctl --user set-environment LC_ALL=C")

    BAD = re.compile(r"^\s*(export\s+)?LC_ALL=[\"']?(C|POSIX)[\"']?\s*$", re.M)

    def check(self, vm, final):
        in_file = bool(self.BAD.search(vm.out_of(f"cat {AIZOME_ENV} 2>/dev/null")))
        in_manager = bool(self.BAD.search(vm.alice("systemctl --user show-environment")[1]))
        return not in_file and not in_manager, {"file-clean": not in_file, "manager-clean": not in_manager}

    def oracle(self, vm):
        vm.alice("sed -i '/^LC_ALL=/d' ~/.config/aizome/environment && systemctl --user unset-environment LC_ALL")


class SpeakersMuted(Train):
    def setup(self, vm):
        vm.alice("wpctl set-mute @DEFAULT_AUDIO_SINK@ 1 && wpctl set-volume @DEFAULT_AUDIO_SINK@ 0")

    def check(self, vm, final):
        sink = vm.alice("timeout 10 wpctl inspect @DEFAULT_AUDIO_SINK@")[1]
        vol = alice_value(vm, "timeout 10 wpctl get-volume @DEFAULT_AUDIO_SINK@")
        m = re.search(r"Volume:\s*([0-9.]+)", vol)
        level = float(m.group(1)) if m else 0.0
        ok = 'node.name = "eval-speakers"' in sink and "MUTED" not in vol and level >= 0.3
        return ok, {"volume": vol}

    def oracle(self, vm):
        vm.alice("wpctl set-mute @DEFAULT_AUDIO_SINK@ 0 && wpctl set-volume @DEFAULT_AUDIO_SINK@ 0.8")


class ScreenshotsFolderLink(Train):
    def setup(self, vm):
        vm.alice("mkdir -p ~/Pictures && rm -rf ~/Pictures/Screenshots && "
                 "ln -s /run/media/alice/KINGSTON/Screenshots ~/Pictures/Screenshots")

    def check(self, vm, final):
        status, out = vm.alice("aizomectl screenshot", timeout=30)
        saved = vm.ok(f"ls {HOME}/Pictures/Screenshots/ 2>/dev/null | grep -q '\\.png$'")
        return status == 0 and saved, {"screenshot": out.strip()[-300:],
                                       "folder": vm.out_of(f"ls -ld {HOME}/Pictures/Screenshots 2>&1")[-200:]}

    def oracle(self, vm):
        vm.alice("rm ~/Pictures/Screenshots && mkdir ~/Pictures/Screenshots")


ALARM_ID = "5e1f03a2"
ALARMS = {"alarms": [{"id": ALARM_ID, "label": "Work", "hour": 6, "minute": 30, "days": [1, 2, 3, 4, 5],
                      "enabled": True, "wake": False, "sound": "alarm-clock-elapsed", "snooze_minutes": 9}]}

# An active user timer that rings the alarm, at its time.
ALARM_TIMER = r"""
for t in $(systemctl --user list-units --type=timer --state=active --no-legend --plain | awk '{print $1}'); do
  s=$(systemctl --user show -p Triggers --value "$t")
  if systemctl --user show -p ExecStart --value "$s" | grep -q -- "--ring ID" &&
     systemctl --user show -p TimersCalendar --value "$t" | grep -q '06:30'; then
    echo "$t"; exit 0
  fi
done
exit 1
"""


class AlarmNotScheduled(Train):
    def setup(self, vm):
        # Copied over from an old laptop after login: the login's sync
        # didn't see it, and the Clock hasn't been opened since.
        alice_write(vm, HOME + "/.local/share/aizome/tokei/alarms.json", json.dumps(ALARMS, indent=2) + "\n")

    def check(self, vm, final):
        status, out = vm.alice(ALARM_TIMER.replace("ID", ALARM_ID))
        return status == 0, {"timer": out.strip()[-200:],
                             "timers": alice_value(vm, "systemctl --user list-timers --all --no-legend --plain")[-400:]}

    def oracle(self, vm):
        vm.alice("aizome-tokei --sync", timeout=60)


HOTARU = HOME + "/.config/aizome/hotaru.toml"
HOTARU_TOML = """font_size = 15
shell = "/usr/bin/zsh"
copy_on_select = false
"""

# hotaru.toml's shell, if set, must be one alice can run.
HOTARU_SHELL = r"""
import os, shutil, sys, tomllib
try:
    d = tomllib.load(open(sys.argv[1], "rb"))
except FileNotFoundError:
    print("ok: no file"); sys.exit(0)
except Exception as e:
    print("unparsable:", e); sys.exit(1)
shell = d.get("shell")
if shell is None:
    print("ok: unset"); sys.exit(0)
prog = shell.split()[0] if isinstance(shell, str) and shell.strip() else ""
path = os.environ["ALICE_PATH"]
found = shutil.which(prog, path=path) if prog else None
print("shell:", shell, "->", found)
sys.exit(0 if found else 1)
"""


class TerminalShellMissing(Train):
    def setup(self, vm):
        alice_write(vm, HOTARU, HOTARU_TOML)

    def check(self, vm, final):
        path = "/run/wrappers/bin:/home/alice/.nix-profile/bin:/etc/profiles/per-user/alice/bin:" \
               "/nix/var/nix/profiles/default/bin:/run/current-system/sw/bin"
        status, out = vm.sh(f"ALICE_PATH={path} python3 -c {shlex.quote(HOTARU_SHELL)} {HOTARU}")
        try:
            size = toml_value(vm, HOTARU, "font_size")
        except ValueError:
            size = None
        # Alice's other terminal settings stay.
        return status == 0 and size == 15, {"shell": out.strip()[-200:], "font_size": size}

    def oracle(self, vm):
        vm.alice("sed -i '/^shell = /d' ~/.config/aizome/hotaru.toml")


class SshAgentStopped(Train):
    def setup(self, vm):
        vm.alice("systemctl --user stop gcr-ssh-agent.socket gcr-ssh-agent.service")

    def check(self, vm, final):
        status, out = vm.alice("SSH_AUTH_SOCK=/run/user/1000/gcr/ssh timeout 10 ssh-add -l", timeout=30)
        # 0: keys listed; 1: the agent answered with none; 2: no agent.
        return status in (0, 1), {"ssh-add": status, "output": out.strip()[-200:]}

    def oracle(self, vm):
        vm.alice("systemctl --user start gcr-ssh-agent.socket")


USER_DIRS = """# This file is written by xdg-user-dirs-update
XDG_DESKTOP_DIR="$HOME/Schreibtisch"
XDG_DOWNLOAD_DIR="$HOME/Downloads"
XDG_DOCUMENTS_DIR="$HOME/Documents"
XDG_MUSIC_DIR="$HOME/Music"
XDG_PICTURES_DIR="$HOME/Pictures"
XDG_VIDEOS_DIR="$HOME/Videos"
"""

# The desktop folder as the shell reads it (aizome-shell's launch.rs).
DESKTOP_DIR = r"""
import os
home = "/home/alice"
d = home + "/Desktop"
try:
    for line in open(home + "/.config/user-dirs.dirs"):
        if line.lstrip().startswith("XDG_DESKTOP_DIR="):
            d = line.strip()[len("XDG_DESKTOP_DIR="):].strip().strip('"').replace("$HOME", home)
            break
except FileNotFoundError:
    pass
print(d)
"""


class DesktopFolderRenamed(Train):
    def setup(self, vm):
        alice_write(vm, HOME + "/Desktop/todo.txt", "call the bank\nreturn library books\n")
        alice_write(vm, HOME + "/Desktop/party-flyer.txt", "Saturday at 7\n")
        alice_write(vm, HOME + "/.config/user-dirs.dirs", USER_DIRS)
        vm.alice("mkdir -p ~/Schreibtisch")

    def check(self, vm, final):
        d = (vm.out_of(f"python3 -c {shlex.quote(DESKTOP_DIR)}").strip().splitlines() or [HOME + "/Desktop"])[-1]
        ok = vm.ok(f"test -s {shlex.quote(d)}/todo.txt")
        return ok, {"desktop": d}

    def oracle(self, vm):
        vm.alice("sed -i 's|^XDG_DESKTOP_DIR=.*|XDG_DESKTOP_DIR=\"$HOME/Desktop\"|' ~/.config/user-dirs.dirs")


DIAGNOSTICS = [
    CoredumpsFillRoot("root-full-coredumps", "diagnostics", "sekka",
                      "I can't save anything any more: the Text Editor says there's no space left on the device, "
                      "but I've hardly put anything on this computer.",
                      diagnosis=[[r"core ?dumps?", r"coredump"], [r"full", r"space", r"ENOSPC", r"100\s*%"]],
                      topic="disk space"),
    JournalNotStored("journal-not-stored", "diagnostics", "sekka",
                     "The Logs app hasn't shown anything new since this morning, and `journalctl -f` stays empty "
                     "even when things happen, like when I plug something in.",
                     diagnosis=[[r"Storage\s*=\s*none", r"journald\.conf", r"50-save-ssd"]], level="hard",
                     topic="logging"),
    WiredConnectionOff("wired-connection-off", "diagnostics", "sekka",
                       "The internet stopped working after I clicked around in the network menu yesterday. The menu "
                       "bar says I'm not connected, and it doesn't come back by itself.",
                       diagnosis=[[r"Wired connection", r"nmcli", r"NetworkManager", r"eth0"],
                                  [r"autoconnect", r"\bdown\b", r"disconnect", r"deactivat", r"not (connected|active)"]],
                       level="easy", topic="NetworkManager"),
    NsswitchNoDns("nsswitch-no-dns", "diagnostics", "sekka",
                  "Names on my network stopped working on this computer: `ping updates.sekka.test` says 'Name or "
                  "service not known'. But `dig updates.sekka.test` gets the right answer straight away, and my "
                  "other devices are fine.",
                  diagnosis=[[r"nsswitch"], [r"\bdns\b", r"hosts:"]], level="hard", topic="name resolution"),
    GroupsReplaced("groups-replaced", "diagnostics", "sekka",
                   "I followed a forum post to give myself access to the video device, and now `sudo` says I'm not in "
                   "the sudoers file, and changing network settings asks for an administrator's password. I'm the "
                   "only person who uses this computer.",
                   diagnosis=[[r"wheel"], [r"usermod", r"group"]], topic="user accounts"),
    TmpNotWorldWritable("tmp-not-world-writable", "diagnostics", "sekka",
                        "Since I tidied up some folders as root yesterday, lots of things fail with 'Permission "
                        "denied': `mktemp` in my terminal, `sort` on big files, an install script. My own folders "
                        "are fine.",
                        diagnosis=[[r"/tmp"], [r"1777", r"sticky", r"world-writable", r"0?755", r"permission", r"mode"]],
                        level="easy", topic="system directory permissions"),
    PrinterPaused("printer-paused", "diagnostics", "sekka",
                  "My print jobs to the Office printer just sit in the queue and never come out, and now it won't "
                  "even take new ones. The printer itself is fine: my laptop prints to it.",
                  diagnosis=[[r"disabled", r"paused", r"stopped", r"cupsenable", r"not accepting", r"reject",
                              r"cupsaccept"]], topic="printing"),
    ImmutableBudget("immutable-budget", "diagnostics", "sekka",
                    "I can't save changes to Documents/budget.txt any more: the Text Editor says 'Operation not "
                    "permitted'. It's my own file, and other files in the same folder save fine.",
                    diagnosis=[[r"immutable", r"chattr", r"lsattr", r"\+i\b", r"attribute"]], topic="file attributes"),
    CurlProxyLeftover("curl-proxy-leftover", "diagnostics", "sekka",
                      "In my terminal, `curl` can't fetch anything any more, not even http://updates.sekka.test on my "
                      "own network: it says it couldn't connect. It started after I came back from a trip, and my "
                      "other devices are fine.",
                      diagnosis=[[r"curlrc"], [r"proxy"]], topic="per-user network settings"),
    OldRouterRoute("old-router-route", "diagnostics", "sekka",
                   "Since I replaced my old router, this computer can't reach updates.sekka.test or anything else on my "
                   "home network: it says 'No route to host'. My other devices are fine.",
                   diagnosis=[[r"route"], [r"10\.88\.0\.254", r"10\.88\.0\.0/25", r"/25", r"old router", r"gateway"]],
                   topic="routing"),
    NotesSnapshotTimerOff("notes-snapshot-timer-off", "diagnostics", "sekka",
                          "My notes used to be copied into Documents/notes-snapshots every hour, but the newest copy "
                          "there is from yesterday afternoon.",
                          diagnosis=[[r"notes-snapshot"], [r"disabled", r"stopped", r"inactive",
                                                           r"not (enabled|running|active|started)"]],
                          level="easy", topic="systemd timers"),
    PhotoImportTilde("photo-import-tilde", "diagnostics", "sekka",
                     "My photo import (`systemctl --user start photo-import`) has failed every time since I edited its "
                     "settings file to point at my Pictures folder.",
                     diagnosis=[[r"tilde", r"\bexpan", r"literal", r"~/Pictures"],
                                [r"PHOTO_SOURCE", r"photo-import\.env", r"EnvironmentFile"]],
                     topic="systemd environment files"),
    DanglingCommandLink("dangling-command-link", "diagnostics", "sekka",
                        "My `~/.local/bin/notes-export` command stopped working after I tidied up my home folder: it "
                        "says 'No such file or directory', but I can see it right there in ~/.local/bin.",
                        diagnosis=[[r"symlink", r"symbolic link", r"\blink"],
                                   [r"Documents/scripts", r"dangling", r"broken", r"moved", r"/home/alice/scripts",
                                    r"~/scripts", r"target"]],
                        level="easy", topic="symbolic links"),
    SharePortClosed("share-port-closed", "diagnostics", "sekka",
                    "I share my Public folder with my phone through my share-folder service (`python3 -m http.server "
                    "8000`). On this computer http://localhost:8000 works, but my phone on the same network can't open "
                    "http://10.88.0.1:8000: it just times out.",
                    diagnosis=[[r"firewall", r"nixos-fw", r"iptables", r"nft"], [r"8000"]], topic="firewall"),
    BashrcPathOverwritten("bashrc-path-overwritten", "diagnostics", "sekka",
                          "Every command I type in a new terminal window says 'command not found', even `ls`. "
                          "Yesterday I only added my own bin folder to my PATH.",
                          diagnosis=[[r"bashrc"], [r"PATH"]], level="easy", topic="shell startup files"),
    PdfOpensInTextEditor("pdf-opens-in-text-editor", "diagnostics", "aizome",
                         "When I double-click a PDF in Files, it opens in the Text Editor as pages of gibberish instead "
                         "of in the document viewer. Other files open fine.",
                         diagnosis=[[r"mimeapps", r"default app", r"xdg-mime", r"association"],
                                    [r"Sumi", r"Text Editor", r"application/pdf"]], topic="default applications"),
    CalculatorHidden("calculator-hidden", "diagnostics", "aizome",
                     "The Calculator has disappeared: it isn't in the Toolchest any more and the command palette can't "
                     "find it, but I never uninstalled it.",
                     diagnosis=[[r"Soroban\.desktop", r"desktop (entry|file)", r"\.local/share/applications"],
                                [r"Hidden", r"NoDisplay", r"\bhid"]], topic="desktop entries"),
    SessionLocaleC("session-locale-c", "diagnostics", "aizome",
                   "Since I logged in this morning, `ls ~/Documents` in my terminal shows my 'Café receipts.txt' as "
                   "'Caf'$'\\303\\251'' receipts.txt', and some programs complain about the locale. I don't know "
                   "what changed.",
                   diagnosis=[[r"LC_ALL"], [r"aizome/environment", r"environment file", r"set-environment",
                                            r"show-environment"]], level="hard", topic="session environment"),
    SpeakersMuted("speakers-muted", "diagnostics", "aizome",
                  "There's no sound any more, from anything. The sound settings still show my Studio Speakers, and "
                  "they're plugged in.",
                  diagnosis=[[r"mute", r"volume"]], level="easy", topic="audio levels"),
    ScreenshotsFolderLink("screenshots-folder-link", "diagnostics", "aizome",
                          "Screenshots don't save any more. When I take one, a notification says it couldn't be "
                          "saved. They used to go into Pictures/Screenshots.",
                          diagnosis=[[r"symlink", r"symbolic link", r"\blink", r"KINGSTON", r"/run/media", r"USB"],
                                     [r"Screenshots"]], topic="user folders"),
    AlarmNotScheduled("alarm-not-scheduled", "diagnostics", "aizome",
                      "I copied my files over from my old laptop yesterday, Clock alarms included, but my 6:30 work "
                      "alarm didn't ring this morning.",
                      diagnosis=[[r"aizome-alarm", r"timer", r"--sync", r"schedul"]], level="hard",
                      topic="Aizome alarms"),
    TerminalShellMissing("terminal-shell-missing", "diagnostics", "aizome",
                         "Since I set up the terminal the way I have it on my work machine, it shows an error instead "
                         "of a prompt whenever I open it.",
                         diagnosis=[[r"hotaru\.toml", r"\bshell\b"], [r"zsh"]], topic="app settings"),
    SshAgentStopped("ssh-agent-stopped", "diagnostics", "aizome",
                    "`ssh-add` says 'Could not open a connection to your authentication agent', so I have to type my "
                    "key's passphrase every time I connect to my server.",
                    diagnosis=[[r"gcr-ssh-agent", r"ssh-agent", r"\bagent"],
                               [r"stopped", r"inactive", r"not running", r"socket", r"dead"]], level="hard",
                    topic="session services"),
    DesktopFolderRenamed("desktop-folder-renamed", "diagnostics", "aizome",
                         "The files in my Desktop folder don't show on the desktop any more; it's empty. They're still "
                         "there when I look in Files.",
                         diagnosis=[[r"user-dirs", r"XDG_DESKTOP_DIR"], [r"Schreibtisch"]], level="hard",
                         topic="user folders"),
]


# Computer use.


class TerminalFontTask(Train):
    def check(self, vm, final):
        try:
            size = toml_value(vm, HOTARU, "font_size")
        except ValueError as e:
            return False, {"error": str(e)[-200:]}
        return isinstance(size, (int, float)) and abs(size - 14) < 0.6, {"font_size": size}

    def oracle(self, vm):
        alice_write(vm, HOTARU, "font_size = 14\n")


TOKEI_ALARMS = HOME + "/.local/share/aizome/tokei/alarms.json"


class AlarmTask(Train):
    def check(self, vm, final):
        try:
            alarms = json.loads(vm.out_of(f"cat {TOKEI_ALARMS} 2>/dev/null") or "{}").get("alarms", [])
        except json.JSONDecodeError:
            alarms = []
        ok = any(a.get("hour") == 6 and a.get("minute") == 45 and a.get("enabled", True) for a in alarms)
        return ok, {"alarms": [(a.get("hour"), a.get("minute"), a.get("enabled")) for a in alarms]}

    def oracle(self, vm):
        alarms = {"alarms": [{"id": "0badc0de", "label": "", "hour": 6, "minute": 45, "days": [], "enabled": True}]}
        alice_write(vm, TOKEI_ALARMS, json.dumps(alarms) + "\n")
        vm.alice("aizome-tokei --sync", timeout=60)


class TrashTask(Train):
    def check(self, vm, final):
        gone = not vm.ok(f"test -e {HOME}/Downloads/readme.txt")
        trashed = vm.out_of(f"grep -l 'not much here' {HOME}/.local/share/Trash/files/* 2>/dev/null").strip()
        return gone and bool(trashed), {"gone": gone, "trash": trashed[-200:]}

    def oracle(self, vm):
        vm.alice("t=~/.local/share/Trash; mkdir -p $t/files $t/info && mv ~/Downloads/readme.txt $t/files/ && "
                 "printf '[Trash Info]\\nPath=/home/alice/Downloads/readme.txt\\nDeletionDate=%s\\n' "
                 "\"$(date +%Y-%m-%dT%H:%M:%S)\" > $t/info/readme.txt.trashinfo")


class VolumeTask(Train):
    def check(self, vm, final):
        sink = vm.alice("timeout 10 wpctl inspect @DEFAULT_AUDIO_SINK@")[1]
        vol = alice_value(vm, "timeout 10 wpctl get-volume @DEFAULT_AUDIO_SINK@")
        m = re.search(r"Volume:\s*([0-9.]+)", vol)
        level = float(m.group(1)) if m else -1
        ok = 'node.name = "eval-speakers"' in sink and "MUTED" not in vol and 0.35 <= level <= 0.65
        return ok, {"volume": vol}

    def oracle(self, vm):
        vm.alice("wpctl set-volume @DEFAULT_AUDIO_SINK@ 0.5")


class EditorSaveTask(Train):
    PATH = HOME + "/Documents/meeting.txt"

    def check(self, vm, final):
        text = vm.out_of(f"cat {self.PATH} 2>/dev/null")
        owner = vm.out_of(f"stat -c %U {self.PATH} 2>/dev/null").strip()
        return "Meeting at 3pm" in text and owner == "alice", {"text": text[:200], "owner": owner}

    def oracle(self, vm):
        alice_write(vm, self.PATH, "Meeting at 3pm\n")


KOYOMI = HOME + "/.local/share/aizome/koyomi"
DENTIST_ICS = """BEGIN:VCALENDAR
VERSION:2.0
PRODID:-//eval//oracle//EN
X-WR-CALNAME:Personal
BEGIN:VEVENT
UID:eval-dentist@studio
DTSTAMP:20261105T000000Z
DTSTART;TZID=America/Chicago:20261112T100000
DTEND;TZID=America/Chicago:20261112T110000
SUMMARY:Dentist
END:VEVENT
END:VCALENDAR
"""


# Events named Dentist, by when they start in alice's zone ("MMDD HH:MM").
DENTIST_EVENTS = r"""
import datetime as dt, glob, re
from zoneinfo import ZoneInfo
here = ZoneInfo("America/Chicago")
for path in glob.glob("/home/alice/.local/share/aizome/koyomi/*.ics"):
    text = re.sub(r"\r?\n[ \t]", "", open(path, encoding="utf-8", errors="replace").read())
    for ev in re.findall(r"BEGIN:VEVENT(.*?)END:VEVENT", text, re.S):
        if not re.search(r"^SUMMARY[^:\n]*:.*dentist", ev, re.I | re.M):
            continue
        m = re.search(r"^DTSTART([^:\n]*):(\d{8})(?:T(\d{4}))?(\d{2})?(Z?)", ev, re.M)
        if not m:
            continue
        params, day, hm, _, z = m.groups()
        t = dt.datetime.strptime(day + (hm or "0000"), "%Y%m%d%H%M")
        tz = re.search(r"TZID=([^;:]+)", params)
        if z:
            t = t.replace(tzinfo=dt.timezone.utc).astimezone(here)
        elif tz:
            t = t.replace(tzinfo=ZoneInfo(tz.group(1))).astimezone(here)
        print(t.strftime("%m%d %H:%M"), path)
"""


class CalendarTask(Train):
    def check(self, vm, final):
        events = vm.out_of(f"python3 -c {shlex.quote(DENTIST_EVENTS)}").strip().splitlines()
        return any(e.startswith("1112 10:00") for e in events), {"events": events[-5:]}

    def oracle(self, vm):
        alice_write(vm, KOYOMI + "/personal.ics", DENTIST_ICS)


class DiskUsageTask(Train):
    def setup(self, vm):
        vm.alice("mkdir -p ~/Videos && head -c 40000000 /dev/urandom > ~/Videos/holiday-2026.mp4 && "
                 "head -c 25000000 /dev/urandom > ~/Videos/birthday.mp4")
        watch_app(vm, "kura", "aizome-kura")

    def check(self, vm, final):
        answer = answer_of(final)
        ran = app_ran(vm, "kura")
        return bool(re.search(r"\bVideos\b", answer)) and ran, {"answer": answer[:200], "disk-usage-ran": ran}

    def oracle(self, vm):
        vm.alice("aizomectl spawn -- aizome-kura")
        vm.sh("sleep 4")
        return {"answer": "Videos", "done": True}


class PinNoteTask(Train):
    def check(self, vm, final):
        try:
            index = json.loads(vm.out_of(f"cat {HOME}/Notes/.fusen.json 2>/dev/null") or "{}")
        except json.JSONDecodeError:
            index = {}
        notes = index.get("notes", {}) if isinstance(index, dict) else {}
        pinned = any("Groceries" in k and isinstance(v, dict) and v.get("pinned") for k, v in notes.items())
        return pinned, {"notes": notes}

    def oracle(self, vm):
        alice_write(vm, HOME + "/Notes/.fusen.json", json.dumps({"notes": {"Groceries.md": {"pinned": True}}}) + "\n")


COMPUTER_USE = [
    TerminalFontTask("cu-terminal-font-size", "computer-use", "aizome",
                     "In the terminal app's preferences, make its text size 14 points.", topic="app preferences"),
    AlarmTask("cu-alarm", "computer-use", "aizome", "In the Clock app, set an alarm for 6:45 in the morning.",
              topic="clock"),
    TrashTask("cu-trash-readme", "computer-use", "aizome",
              "Using the Files app, move readme.txt from the Downloads folder to the Trash.", topic="files"),
    VolumeTask("cu-volume-half", "computer-use", "aizome", "Turn the sound volume down to about half.",
               topic="sound"),
    EditorSaveTask("cu-editor-save", "computer-use", "aizome",
                   "Open the Text Editor, write 'Meeting at 3pm', and save it as meeting.txt in the Documents folder.",
                   topic="text editor"),
    CalendarTask("cu-calendar-event", "computer-use", "aizome",
                 "In the Calendar app, add an event called Dentist on 12 November at 10:00 in the morning.",
                 level="hard", topic="calendar"),
    DiskUsageTask("cu-disk-usage", "computer-use", "aizome",
                  "Use the Disk Usage app to find out which folder in my home folder takes up the most space, and "
                  "tell me its name.", topic="disk usage"),
    PinNoteTask("cu-pin-note", "computer-use", "aizome", "In the Notes app, pin the Groceries note.", topic="notes"),
]

SCENARIOS = {s.id: s for s in DIAGNOSTICS + COMPUTER_USE}
