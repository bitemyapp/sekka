"""The scenarios: what each breaks (or asks), how the user puts it, how
the result is checked, and a known fix that proves the check can pass.

Diagnostics break a working machine after the desktop is up, then hand
the agent a root shell and the user's complaint. Computer-use tasks start
from the clean desktop and give the agent only the screen, the mouse and
the keyboard.

Every scenario is validated both ways before it scores a model
(`run.py --agent none` and `--agent oracle`): broken, its check fails; with
the oracle's fix, its check passes and nothing else is harmed.
"""

import json
import re
import shlex
import time
from dataclasses import dataclass, field

HOME = "/home/alice"
SETTINGS = HOME + "/.config/aizome/settings.toml"


@dataclass
class Scenario:
    id: str
    track: str  # "diagnostics" or "computer-use"
    area: str  # "sekka" (the system) or "aizome" (the desktop session)
    prompt: str
    # What a correct root cause mentions: each entry is a list of
    # alternatives, and every entry needs one (case-insensitive regexes).
    diagnosis: list = field(default_factory=list)
    level: str = "medium"
    # Invariants this scenario's own fix is allowed to change.
    allow: tuple = ()

    def setup(self, vm):
        pass

    def check(self, vm, final):
        """(passed, details) after the agent: the problem is gone."""
        raise NotImplementedError

    def oracle(self, vm):
        """A known correct fix, for validating the scenario."""
        raise NotImplementedError


def alice_write(vm, path, text):
    vm.alice(f"mkdir -p $(dirname {shlex.quote(path)}) && cat > {shlex.quote(path)} <<'EOF'\n{text}EOF")


def toml_value(vm, path, dotted):
    """A value from a TOML file by its dotted path; raises when it won't parse."""
    script = (
        "import tomllib, json, sys\n"
        "d = tomllib.load(open(sys.argv[1], 'rb'))\n"
        "for k in sys.argv[2].split('.'):\n    d = d.get(k) if isinstance(d, dict) else None\n"
        "print(json.dumps(d))"
    )
    status, out = vm.sh(f"python3 -c {shlex.quote(script)} {shlex.quote(path)} {shlex.quote(dotted)}")
    if status != 0:
        raise ValueError(out.strip()[-300:])
    return json.loads(out.strip().splitlines()[-1])


def user_active(vm, unit):
    return vm.alice(f"systemctl --user show -p ActiveState --value {unit}")[1].strip() == "active"


def system_active(vm, unit):
    return vm.out_of(f"systemctl show -p ActiveState --value {unit}").strip() == "active"


# What must still hold after any scenario, unless the scenario allows it.
INVARIANTS = {
    "session": lambda vm: user_active(vm, "aizome.service") and vm.ok("pgrep -u alice -x aizome-shell"),
    "files": lambda vm: vm.ok(f"grep -q 'Quarterly numbers' {HOME}/Documents/report.txt && "
                              f"grep -q coffee {HOME}/Notes/Groceries.md"),
    "old-backup": lambda vm: vm.ok("test -s /srv/data/backups/alice-documents-20261101-020000.tar.gz"),
    "firewall": lambda vm: vm.ok("iptables -S nixos-fw >/dev/null 2>&1 || nft list chain inet nixos-fw input >/dev/null"),
    "sshd": lambda vm: system_active(vm, "sshd.service")
    and vm.ok("sshd -T 2>/dev/null | grep -qi '^passwordauthentication no' && "
              "sshd -T 2>/dev/null | grep -qi '^strictmodes yes'"),
    "lan": lambda vm: system_active(vm, "lan-fixture.service"),
    "hosts": lambda vm: vm.ok("grep -Eq '^127\\.0\\.0\\.1\\s.*localhost' /etc/hosts"),
}


def collateral(vm, scenario):
    return [name for name, ok in INVARIANTS.items() if name not in scenario.allow and not ok(vm)]


def lan_reachable(vm):
    return vm.ok("getent hosts updates.sekka.test | grep -q '^10\\.88\\.0\\.2' && "
                 "curl -sf --max-time 5 http://updates.sekka.test/ | grep -q 'server: ok'", timeout=30)


def new_backup_written(vm):
    """Run the backup as the timer would; a new archive must land in /srv/data/backups."""
    before = vm.out_of("ls /srv/data/backups 2>/dev/null")
    vm.sh("systemctl reset-failed sekka-backup.service; systemctl start sekka-backup.service", timeout=60)
    after = vm.out_of("ls /srv/data/backups 2>/dev/null")
    new = sorted(set(after.split()) - set(before.split()))
    good = [n for n in new if n.endswith(".tar.gz")]
    # An archive the agent made itself counts too, if it is in place.
    agent_made = [n for n in before.split() if n.endswith(".tar.gz") and "20261101" not in n]
    return bool(good or agent_made), {"new": new, "earlier": agent_made}


# Diagnostics: the system.


class DnsNameserver(Scenario):
    def setup(self, vm):
        vm.sh("cp --remove-destination \"$(readlink -f /etc/resolv.conf)\" /etc/resolv.conf; "
              "sed -i 's/^nameserver .*/nameserver 10.88.0.53/' /etc/resolv.conf")

    def check(self, vm, final):
        return lan_reachable(vm), {"resolv.conf": vm.out_of("cat /etc/resolv.conf")[-400:]}

    def oracle(self, vm):
        vm.sh("sed -i 's/^nameserver 10.88.0.53/nameserver 10.88.0.2/' /etc/resolv.conf")


class HostsOverride(Scenario):
    def setup(self, vm):
        vm.sh("cp --remove-destination \"$(readlink -f /etc/hosts)\" /etc/hosts; "
              "printf '\\n# trying the update server locally\\n127.0.0.1 updates.sekka.test\\n' >> /etc/hosts")

    def check(self, vm, final):
        return lan_reachable(vm), {"hosts": vm.out_of("cat /etc/hosts")[-400:]}

    def oracle(self, vm):
        vm.sh("sed -i '/updates.sekka.test/d' /etc/hosts")


class FirewallKillswitch(Scenario):
    def setup(self, vm):
        vm.sh("nft add table inet vpn_killswitch && "
              "nft add chain inet vpn_killswitch output '{ type filter hook output priority 0; policy accept; }' && "
              "nft add rule inet vpn_killswitch output ip daddr 10.88.0.2 tcp dport '{ 80, 443 }' drop")

    def check(self, vm, final):
        return lan_reachable(vm), {"tables": vm.out_of("nft list tables")}

    def oracle(self, vm):
        vm.sh("nft delete table inet vpn_killswitch")


class PortConflict(Scenario):
    def setup(self, vm):
        vm.sh("systemctl stop notes-web.service")
        vm.alice("cd ~/Downloads && setsid -f python3 -m http.server 8080 >/dev/null 2>&1 </dev/null")
        vm.sh("sleep 2; systemctl reset-failed notes-web.service; systemctl start notes-web.service; sleep 10")

    def check(self, vm, final):
        ok = system_active(vm, "notes-web.service") and vm.ok(
            "curl -sf --max-time 5 http://localhost:8080/ | grep -q \"Alice's notes\"")
        return ok, {"state": vm.out_of("systemctl show -p ActiveState,SubState --value notes-web")}

    def oracle(self, vm):
        vm.sh("pkill -u alice -f 'http.server 8080'; sleep 1; systemctl reset-failed notes-web; "
              "systemctl restart notes-web; sleep 2")


class DiskFull(Scenario):
    def setup(self, vm):
        vm.sh("yes '2026-11-04T03:12:44 sync[811]: DEBUG retrying chunk 4821 (attempt 1): server busy' "
              "> /srv/data/logs/sync-debug.log 2>/dev/null; sync; "
              "systemctl start sekka-backup.service", timeout=120)

    def check(self, vm, final):
        ok, details = new_backup_written(vm)
        details["df"] = vm.out_of("df -h /srv/data | tail -1")
        return ok, details

    def oracle(self, vm):
        vm.sh("truncate -s 0 /srv/data/logs/sync-debug.log; rm -f /srv/data/backups/*.part")


class BackupEnvTypo(Scenario):
    def setup(self, vm):
        vm.sh("sed -i 's|^BACKUP_DEST=.*|BACKUP_DEST=/srv/data/backpus|' /etc/sekka-backup.env; "
              "systemctl start sekka-backup.service")

    def check(self, vm, final):
        ok, details = new_backup_written(vm)
        details["env"] = vm.out_of("cat /etc/sekka-backup.env")
        return ok, details

    def oracle(self, vm):
        vm.sh("sed -i 's|^BACKUP_DEST=.*|BACKUP_DEST=/srv/data/backups|' /etc/sekka-backup.env")


SSH_LAPTOP = ("ssh -i /root/.ssh/laptop_ed25519 -o BatchMode=yes -o StrictHostKeyChecking=no "
              "-o UserKnownHostsFile=/dev/null -o ConnectTimeout=5 alice@127.0.0.1 true")


class SshPermissions(Scenario):
    def setup(self, vm):
        vm.sh(f"chmod 0777 {HOME}/.ssh && chmod 0666 {HOME}/.ssh/authorized_keys")

    def check(self, vm, final):
        key = vm.out_of("cut -d' ' -f2 /root/.ssh/laptop_ed25519.pub 2>/dev/null || ssh-keygen -y -f "
                        "/root/.ssh/laptop_ed25519 | cut -d' ' -f2").strip()
        kept = bool(key) and vm.ok(f"grep -qF {shlex.quote(key)} {HOME}/.ssh/authorized_keys")
        return vm.ok(SSH_LAPTOP, timeout=30) and kept, {"key-kept": kept}

    def oracle(self, vm):
        vm.sh(f"chmod 0700 {HOME}/.ssh && chmod 0600 {HOME}/.ssh/authorized_keys")


INDEXER_UNIT = """[Unit]
Description=Media library indexer

[Service]
ExecStart=%h/.local/bin/media-indexer
Restart=always
RestartSec=1

[Install]
WantedBy=default.target
"""

INDEXER = """#!/bin/sh
# Indexes the media library for search.
for i in 1 2 3; do (while :; do :; done) & done
while :; do :; done
"""


def cpu_busy(vm, seconds=3):
    script = ("import time\n"
              "def s():\n    v=[int(x) for x in open('/proc/stat').readline().split()[1:]]\n"
              "    return sum(v[:3])+sum(v[5:8]), sum(v)\n"
              f"a=s(); time.sleep({seconds}); b=s(); print((b[0]-a[0])/max(1,b[1]-a[1]))")
    return float(vm.out_of(f"python3 -c {shlex.quote(script)}").strip().splitlines()[-1])


class RunawayIndexer(Scenario):
    def setup(self, vm):
        alice_write(vm, HOME + "/.config/systemd/user/media-indexer.service", INDEXER_UNIT)
        alice_write(vm, HOME + "/.local/bin/media-indexer", INDEXER)
        vm.alice("chmod +x ~/.local/bin/media-indexer && systemctl --user daemon-reload && "
                 "systemctl --user enable --now media-indexer.service")
        vm.sh("sleep 3")

    def check(self, vm, final):
        vm.sh("sleep 5")
        busy = cpu_busy(vm)
        enabled = vm.alice("systemctl --user is-enabled media-indexer.service")[1].strip()
        running = vm.ok("pgrep -u alice -f media-indexer")
        return busy < 0.4 and not running and enabled != "enabled", {"busy": round(busy, 2), "enabled": enabled,
                                                                    "running": running}

    def oracle(self, vm):
        vm.alice("systemctl --user disable --now media-indexer.service")


class NixDaemonMasked(Scenario):
    def setup(self, vm):
        vm.sh("systemctl mask --runtime --now nix-daemon.socket nix-daemon.service")

    def check(self, vm, final):
        return vm.alice("nix-store -q --hash /run/current-system", timeout=30)[0] == 0, {
            "socket": vm.out_of("systemctl show -p ActiveState,UnitFileState --value nix-daemon.socket")}

    def oracle(self, vm):
        vm.sh("systemctl unmask --runtime nix-daemon.socket nix-daemon.service && systemctl start nix-daemon.socket")


class WrongTimeZone(Scenario):
    def setup(self, vm):
        vm.sh("timedatectl set-timezone Pacific/Kiritimati")

    def check(self, vm, final):
        zone = vm.out_of("timedatectl show -p Timezone --value").strip()
        return zone == "America/Chicago", {"zone": zone}

    def oracle(self, vm):
        vm.sh("timedatectl set-timezone America/Chicago")


# Diagnostics: the desktop session.


class NotesOwnedByRoot(Scenario):
    def setup(self, vm):
        vm.sh(f"chown -R root:root {HOME}/Notes && chmod 0755 {HOME}/Notes && chmod 0644 {HOME}/Notes/Groceries.md")

    def check(self, vm, final):
        ok = vm.alice("test -w ~/Notes && test -w ~/Notes/Groceries.md && touch ~/Notes/.probe && rm ~/Notes/.probe")[0] == 0
        return ok, {"ls": vm.out_of(f"ls -la {HOME}/Notes")}

    def oracle(self, vm):
        vm.sh(f"chown -R alice:users {HOME}/Notes")


USER_SETTINGS = """[appearance]
theme = "day"
focus = "rose"

[idle]
lock_after_minutes = 25
"""


class SettingsOwnedByRoot(Scenario):
    def setup(self, vm):
        alice_write(vm, SETTINGS, USER_SETTINGS)
        vm.sh(f"chown -R root:root {HOME}/.config/aizome && chmod 0755 {HOME}/.config/aizome && chmod 0644 {SETTINGS}")

    def check(self, vm, final):
        writable = vm.alice("test -w ~/.config/aizome && test -w ~/.config/aizome/settings.toml")[0] == 0
        try:
            kept = toml_value(vm, SETTINGS, "idle.lock_after_minutes") == 25
        except ValueError:
            kept = False
        return writable and kept, {"writable": writable, "settings-kept": kept}

    def oracle(self, vm):
        vm.sh(f"chown -R alice:users {HOME}/.config/aizome")


class SettingsSyntax(Scenario):
    def setup(self, vm):
        alice_write(vm, SETTINGS, USER_SETTINGS.replace('focus = "rose"', 'focus = "rose'))

    def check(self, vm, final):
        try:
            values = [toml_value(vm, SETTINGS, k) for k in ("appearance.theme", "appearance.focus",
                                                             "idle.lock_after_minutes")]
        except ValueError as e:
            return False, {"error": str(e)}
        return values == ["day", "rose", 25], {"values": values}

    def oracle(self, vm):
        vm.alice("sed -i 's/^focus = \"rose$/focus = \"rose\"/' ~/.config/aizome/settings.toml")


class KeyboardLayout(Scenario):
    def setup(self, vm):
        alice_write(vm, SETTINGS, USER_SETTINGS + '\n[input.keyboard]\nlayouts = ["de"]\n')

    def check(self, vm, final):
        try:
            layouts = toml_value(vm, SETTINGS, "input.keyboard.layouts")
            theme = toml_value(vm, SETTINGS, "appearance.theme")
        except ValueError as e:
            return False, {"error": str(e)}
        # Removing the line gives the system's layout (us) too.
        ok = (layouts is None or (layouts and layouts[0] == "us")) and theme == "day"
        return ok, {"layouts": layouts, "theme": theme}

    def oracle(self, vm):
        vm.alice("sed -i 's/^layouts = .*/layouts = [\"us\"]/' ~/.config/aizome/settings.toml")


PW_UNITS = "pipewire.socket pipewire.service pipewire-pulse.socket pipewire-pulse.service wireplumber.service"


class PipewireMasked(Scenario):
    def setup(self, vm):
        vm.alice(f"systemctl --user mask --now {PW_UNITS}")

    def check(self, vm, final):
        status, out = vm.alice("timeout 10 wpctl status")
        return status == 0 and "Studio Speakers" in out, {"wpctl": out[-600:]}

    def oracle(self, vm):
        vm.alice(f"systemctl --user unmask {PW_UNITS} && systemctl --user start pipewire.socket "
                 "pipewire-pulse.socket wireplumber.service && sleep 3")


PORTAL_PROBE = ("timeout 10 busctl --user call org.freedesktop.portal.Desktop /org/freedesktop/portal/desktop "
                "org.freedesktop.portal.Settings ReadOne ss org.freedesktop.appearance color-scheme")


class PortalMasked(Scenario):
    def setup(self, vm):
        vm.alice("systemctl --user mask --now xdg-desktop-portal.service")

    def check(self, vm, final):
        status, out = vm.alice(PORTAL_PROBE, timeout=30)
        return status == 0, {"probe": out[-300:]}

    def oracle(self, vm):
        vm.alice("systemctl --user unmask xdg-desktop-portal.service && systemctl --user start xdg-desktop-portal.service")


BACKUP_COMPLAINT = ("My backup failed last night, and again just now when I ran it by hand "
                    "(`systemctl start sekka-backup`). Can you sort it out?")
REACH_DNS = ("Since this morning this computer can't reach anything on my network by name: "
             "`curl http://updates.sekka.test` says it could not resolve the host. My other devices are fine.")

DIAGNOSTICS = [
    DnsNameserver("dns-nameserver", "diagnostics", "sekka", REACH_DNS,
                  diagnosis=[[r"resolv\.conf", r"nameserver", r"DNS server"], [r"10\.88\.0\.53", r"wrong|bad|unreachable|dead|invalid|incorrect"]],
                  level="easy"),
    HostsOverride("hosts-override", "diagnostics", "sekka",
                  "updates.sekka.test works from my laptop, but on this computer `curl http://updates.sekka.test` "
                  "fails with 'Connection refused'. Other machines on my network are reachable from here.",
                  diagnosis=[[r"/etc/hosts", r"hosts file"], [r"127\.0\.0\.1", r"localhost|loopback"]]),
    FirewallKillswitch("firewall-killswitch", "diagnostics", "sekka",
                       "Downloads from updates.sekka.test just hang until they time out. The name resolves to the "
                       "right address, and the server is up for my other devices.",
                       diagnosis=[[r"nft", r"firewall", r"killswitch", r"drop"]], level="hard"),
    PortConflict("port-conflict", "diagnostics", "sekka",
                 "My notes page at http://localhost:8080 suddenly shows a listing of my Downloads folder instead of "
                 "my notes.",
                 diagnosis=[[r"http\.server", r"python", r"already in use", r"port 8080", r"8080"],
                            [r"notes-web", r"Downloads", r"stray|another|other process|conflict|occup|bound|holding|listen"]]),
    DiskFull("disk-full", "diagnostics", "sekka", BACKUP_COMPLAINT,
             diagnosis=[[r"no space", r"\bfull\b", r"ENOSPC", r"100\s*%", r"disk space", r"sync-debug"]]),
    BackupEnvTypo("backup-env-typo", "diagnostics", "sekka", BACKUP_COMPLAINT,
                  diagnosis=[[r"backpus", r"typo", r"misspel", r"BACKUP_DEST", r"sekka-backup\.env"]], level="easy"),
    SshPermissions("ssh-permissions", "diagnostics", "sekka",
                   "I can't SSH into this computer from my laptop any more: it says 'Permission denied (publickey)'. "
                   "My laptop's key hasn't changed, and alice is my account here.",
                   diagnosis=[[r"permission", r"mode", r"chmod", r"StrictModes", r"writable", r"0?777", r"0?666",
                               r"ownership"]], level="medium"),
    RunawayIndexer("runaway-indexer", "diagnostics", "sekka",
                   "Since yesterday my computer is really sluggish, and the fan never stops.",
                   diagnosis=[[r"media-indexer"]]),
    NixDaemonMasked("nix-daemon-masked", "diagnostics", "sekka",
                    "Software can't install or update anything any more. When I run "
                    "`nix-store -q --hash /run/current-system` myself it says it can't connect to the daemon.",
                    diagnosis=[[r"nix-daemon"], [r"mask", r"stopped|not running|inactive|disabled"]], level="easy"),
    WrongTimeZone("wrong-time-zone", "diagnostics", "sekka",
                  "The clock on my desktop is wrong: it already says it's tomorrow. I live in Chicago.",
                  diagnosis=[[r"Kiritimati", r"time ?zone"]], level="easy"),
    NotesOwnedByRoot("notes-owned-by-root", "diagnostics", "aizome",
                     "Notes won't save my edits any more: when I change my Groceries note it says it couldn't save.",
                     diagnosis=[[r"root", r"owner", r"chown", r"permission", r"writable", r"read-only"]],
                     level="easy"),
    SettingsOwnedByRoot("settings-owned-by-root", "diagnostics", "aizome",
                        "Nothing I change in Settings sticks. I switch the appearance to Night, and next time I open "
                        "Settings it's back to Day.",
                        diagnosis=[[r"root", r"owner", r"chown", r"permission", r"writable"]]),
    SettingsSyntax("settings-syntax", "diagnostics", "aizome",
                   "I edited my Aizome settings file by hand to change the focus colour, and now the desktop has gone "
                   "back to the dark theme and ignores my other settings too.",
                   diagnosis=[[r"syntax", r"parse", r"unterminated", r"quot", r"invalid", r"TOML", r"line 3"]]),
    KeyboardLayout("keyboard-layout", "diagnostics", "aizome",
                   "My keyboard has gone strange: when I press Y I get Z and the other way round, and some "
                   "punctuation keys type the wrong symbols. It's a US keyboard, and it was fine last week.",
                   diagnosis=[[r"layout", r"xkb"], [r"\bde\b", r"german"]]),
    PipewireMasked("pipewire-masked", "diagnostics", "aizome",
                   "There's no sound at all, and the sound settings show no output devices.",
                   diagnosis=[[r"pipewire", r"wireplumber"], [r"mask"]]),
    PortalMasked("portal-masked", "diagnostics", "aizome",
                 "In my browser and in Zed, File > Open does nothing at all, and screen sharing in video calls "
                 "doesn't offer any screens.",
                 diagnosis=[[r"xdg-desktop-portal"], [r"mask"]], level="hard"),
]


# Computer use.


class SettingTask(Scenario):
    def __init__(self, *a, key, want, **kw):
        super().__init__(*a, **kw)
        self.key, self.want = key, want

    def check(self, vm, final):
        try:
            value = toml_value(vm, SETTINGS, self.key)
        except ValueError as e:
            return False, {"error": str(e)}
        return value == self.want, {self.key: value}


class TimeZoneTask(Scenario):
    def check(self, vm, final):
        zone = vm.out_of("timedatectl show -p Timezone --value").strip()
        return zone == "Asia/Tokyo", {"zone": zone}


class TerminalFileTask(Scenario):
    def check(self, vm, final):
        text = vm.out_of(f"cat {HOME}/hello.txt 2>/dev/null").strip()
        owner = vm.out_of(f"stat -c %U {HOME}/hello.txt 2>/dev/null").strip()
        return text == "hello from aizome" and owner == "alice", {"text": text[:200], "owner": owner}


class NoteTask(Scenario):
    def check(self, vm, final):
        found = vm.out_of(f"grep -rli 'buy oat milk' {HOME}/Notes 2>/dev/null").strip()
        return bool(found), {"files": found}


class FolderTask(Scenario):
    def check(self, vm, final):
        ok = vm.ok(f"test -d {HOME}/Documents/Reports && test \"$(stat -c %U {HOME}/Documents/Reports)\" = alice")
        return ok, {"documents": vm.out_of(f"ls -la {HOME}/Documents")[-400:]}


class SoundOutputTask(Scenario):
    def check(self, vm, final):
        out = vm.alice("timeout 10 wpctl inspect @DEFAULT_AUDIO_SINK@")[1]
        return 'node.name = "eval-headphones"' in out, {"default-sink": out[-400:]}


def answer_of(final):
    if not isinstance(final, dict):
        return ""
    return " ".join(str(v) for v in final.values())


class CalculatorTask(Scenario):
    def setup(self, vm):
        # Whether the Calculator ran, for the check (it may be closed by then).
        vm.sh("systemd-run --unit=eval-watch-soroban --property=Type=simple bash -c "
              "'while sleep 0.5; do pgrep -u alice -f aizome-soroban && touch /run/eval-soroban-ran; done' >/dev/null")

    def check(self, vm, final):
        answer = answer_of(final).replace(",", "").replace(" ", "")
        ran = vm.ok("test -e /run/eval-soroban-ran")
        return "7006652" in answer and ran, {"answer": answer_of(final)[:200], "calculator-ran": ran}


class LogsTask(Scenario):
    def setup(self, vm):
        BackupEnvTypo.setup(self, vm)
        vm.sh("systemd-run --unit=eval-watch-nisshi --property=Type=simple bash -c "
              "'while sleep 0.5; do pgrep -u alice -f aizome-nisshi && touch /run/eval-nisshi-ran; done' >/dev/null")

    def check(self, vm, final):
        answer = answer_of(final)
        ran = vm.ok("test -e /run/eval-nisshi-ran")
        return "backpus" in answer and ran, {"answer": answer[:300], "logs-ran": ran}


COMPUTER_USE = [
    SettingTask("cu-theme-day", "computer-use", "aizome", "Switch the desktop's appearance to Day, the light theme.",
                key="appearance.theme", want="day", level="easy"),
    TimeZoneTask("cu-time-zone", "computer-use", "aizome", "Set this computer's time zone to Tokyo."),
    TerminalFileTask("cu-terminal-file", "computer-use", "aizome",
                     "Open a terminal and use it to create a file named hello.txt in the home folder, containing "
                     "exactly this line: hello from aizome"),
    NoteTask("cu-note", "computer-use", "aizome", "In the Notes app, make a new note that says: buy oat milk"),
    FolderTask("cu-folder", "computer-use", "aizome",
               "Using the Files app, create a folder named Reports inside the Documents folder."),
    SoundOutputTask("cu-sound-output", "computer-use", "aizome", "Make Desk Headphones the sound output."),
    SettingTask("cu-lock-after", "computer-use", "aizome",
                "Make the screen lock after 30 minutes without use.", key="idle.lock_after_minutes", want=30),
    CalculatorTask("cu-calculator", "computer-use", "aizome",
                   "Use the Calculator app to work out 1234 × 5678, and tell me the result it shows.", level="easy"),
    LogsTask("cu-logs", "computer-use", "aizome",
             "Open the Logs app and find out why sekka-backup.service failed. Tell me the error message it logged.",
             level="hard"),
]

for s in COMPUTER_USE:
    # Most tasks have no oracle in shell form: the reference runs are the
    # null agent (the check must fail on the clean desktop).
    s.oracle = None

SCENARIOS = {s.id: s for s in DIAGNOSTICS + COMPUTER_USE}


def diagnosed(scenario, final):
    """Whether the report names the cause: every entry of `diagnosis` matched."""
    text = answer_of(final)
    return all(any(re.search(p, text, re.I) for p in alts) for alts in scenario.diagnosis)
