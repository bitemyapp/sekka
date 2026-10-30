//! What Nisshi knows of the programs that write to the journal: for each,
//! the topics its messages are about, each a pattern over the message
//! ("Wi-Fi" in NetworkManager's, "Failed logins" in sshd's). Suggestions
//! offer a program's topics when its messages are selected, counted, and
//! only those that find something.

use std::sync::{Arc, LazyLock};

use regex::Regex;

use crate::entry::{Entry, Transport};

pub struct Known {
    /// What it's called.
    pub name: &'static str,
    /// The sources it writes as (lower case; `*` at the end: any that
    /// start so).
    pub sources: &'static [&'static str],
    /// (topic, pattern over the message).
    pub topics: &'static [(&'static str, &'static str)],
}

pub static KNOWN: &[Known] = &[
    Known {
        name: "the kernel",
        sources: &["kernel"],
        topics: &[
            ("USB devices", r"(?i)\busb\b|\busb \d|new (high|full|super|low)-speed|usbcore|xhci"),
            (
                "Storage errors",
                r"(?i)I/O error|blk_update_request|critical medium error|EXT4-fs (error|warning)|BTRFS (error|warning|critical)|XFS.*(error|corrupt)|nvme.*(timeout|reset|error|abort)|ata\d+.*(failed|error|exception|frozen)|Buffer I/O",
            ),
            ("Out of memory", r"(?i)out of memory|oom-kill|oom_reaper|invoked oom-killer|Killed process"),
            ("Graphics drivers", r"(?i)amdgpu|nvidia|nouveau|i915|\bxe\b|\[drm\]|drm:|radeon|simpledrm|gpu"),
            (
                "Crashes and traps",
                r"segfault at|^traps: |general protection fault|Oops|BUG: |Call Trace|watchdog: BUG|soft lockup|hung_task",
            ),
            (
                "Network links",
                r"(?i)link (is )?(up|down)|link becomes ready|carrier|renamed from|entered (promiscuous|forwarding|disabled) state",
            ),
            (
                "Wi-Fi",
                r"(?i)wlan|\bwlp|iwlwifi|ath\d+k|mt79|rtw|brcmf|cfg80211|deauthenticat|associat|authenticate with",
            ),
            ("Bluetooth", r"(?i)bluetooth|\bhci\d|btusb|rfcomm"),
            ("Firmware and ACPI", r"(?i)\bACPI\b|firmware|microcode|\bBIOS\b|\bDMI\b|EFI"),
            (
                "Sleep and wake",
                r"(?i)PM: (suspend|resume|hibernation)|suspend (entry|exit)|Freezing user space|Restarting tasks|ACPI: (PM|Waking)|s2idle",
            ),
            ("Temperature", r"(?i)thermal|temperature|throttl|overheat"),
            ("Security modules", r"(?i)audit:|apparmor|selinux|landlock|lockdown"),
        ],
    },
    Known {
        name: "systemd",
        sources: &["systemd", "init"],
        topics: &[
            (
                "Failed units",
                r"(?i)failed|entered failed state|Failed with result|Main process exited, code=(exited|killed|dumped), status=[^0]",
            ),
            ("Started and stopped", r"^(Started|Stopped|Starting|Stopping|Finished|Reloaded|Reloading|Deactivated) "),
            ("Restarts", r"(?i)Scheduled restart job|restart counter|start request repeated too quickly"),
            ("Timeouts", r"(?i)timed out|timeout|took too long|Killing process"),
            ("Boot milestones", r"^(Reached target|Startup finished|Queued start job|Listening on|Mounted|Mounting)"),
            ("Timers", r"(?i)\.timer\b|Triggering"),
            ("Resource use", r"Consumed .*(CPU time|memory)"),
        ],
    },
    Known {
        name: "NetworkManager",
        sources: &["networkmanager", "nm-dispatcher", "nm-openvpn", "nm-wireguard"],
        topics: &[
            (
                "Connection changes",
                r"(?i)state change|\bActivation\b|activation: |device \([^)]+\): state|connectivity",
            ),
            ("Wi-Fi", r"(?i)wifi|wi-fi|supplicant|\bSSID\b|associat|802-11|wpa|\bwlan|\bwlp"),
            ("DHCP", r"(?i)\bdhcp"),
            ("DNS", r"(?i)\bdns\b|resolv|nameserver"),
            ("VPN", r"(?i)\bvpn\b|wireguard|openvpn|\bwg\d"),
            ("Ethernet", r"(?i)ethernet|\beth\d|\benp\d|carrier"),
        ],
    },
    Known {
        name: "Wi-Fi",
        sources: &["wpa_supplicant", "iwd"],
        topics: &[
            ("Connections", r"CTRL-EVENT-(CONNECTED|DISCONNECTED|ASSOC)|(?i)\bconnected\b|disconnect|associat"),
            ("Authentication problems", r"(?i)4-way|WRONG_KEY|reason=|TEMP-DISABLED|auth.*(fail|timeout)|handshake"),
            ("Scanning", r"(?i)scan"),
            ("Signal and roaming", r"(?i)roam|\bbss\b|signal|beacon"),
        ],
    },
    Known {
        name: "SSH",
        sources: &["sshd", "sshd-session", "sshd-auth", "ssh"],
        topics: &[
            (
                "Failed logins",
                r"(?i)Failed (password|publickey|none)|Invalid user|authentication failure|Connection closed by authenticating user|maximum authentication attempts|Unable to negotiate",
            ),
            ("Accepted logins", r"Accepted (password|publickey|keyboard-interactive)"),
            ("Disconnections", r"(?i)Disconnected from|Connection closed|Received disconnect|Timeout"),
        ],
    },
    Known {
        name: "administrator access",
        sources: &["sudo", "su", "pkexec", "doas", "polkitd", "polkit-agent-helper-1"],
        topics: &[
            ("Commands run", r"COMMAND="),
            (
                "Wrong passwords",
                r"(?i)incorrect password|authentication failure|not in (the )?sudoers|a password is required|conversation failed|Not authorized",
            ),
            ("Sessions opened and closed", r"(?i)session (opened|closed)"),
            (
                "Authorizations",
                r"(?i)Operator of|Registered Authentication Agent|Unregistered Authentication Agent|authoriz",
            ),
        ],
    },
    Known {
        name: "logins",
        sources: &["systemd-logind", "login", "greetd", "gdm-password", "aizome-greeter", "sddm", "lightdm"],
        topics: &[
            ("Sessions", r"(?i)New session|Removed session|logged out|session (opened|closed)|login"),
            ("Sleep and lid", r"(?i)suspend|sleep|Lid (opened|closed)|Power key|hibernat|Operation '|Delay lock|resum"),
            ("Seats and devices", r"(?i)\bseat|Watching system buttons|power button|input device"),
        ],
    },
    Known {
        name: "Bluetooth",
        sources: &["bluetoothd", "obexd", "blueman*"],
        topics: &[
            ("Devices connecting", r"(?i)connect|paired|pairing|bonding|device"),
            ("Audio", r"(?i)a2dp|hfp|hsp|avdtp|avrcp|sink|endpoint|codec"),
            ("Profiles and plugins", r"(?i)profile|plugin|\bSAP\b|BAP|vcp|mcp"),
        ],
    },
    Known {
        name: "sound",
        sources: &["pipewire", "pipewire-pulse", "wireplumber", "pulseaudio", "alsactl"],
        topics: &[
            ("Devices", r"(?i)\bdevice|\bnode\b|\balsa|\bcard\b|\bsink\b|\bsource\b|profile"),
            ("Bluetooth audio", r"(?i)bluez|bluetooth|a2dp|hfp"),
            ("Dropouts", r"(?i)underrun|overrun|xrun|resync|out of buffers"),
            ("Permissions", r"(?i)permission|portal|access"),
        ],
    },
    Known {
        name: "Nix",
        sources: &["nix-daemon", "determinate-nixd", "nix", "nix-gc", "nix-optimise"],
        topics: &[
            ("Builds", r"(?i)building|\bbuilt\b|builder for|build of|error: builder"),
            ("Downloads", r"(?i)copying path|download|substitut|querying|fetching"),
            ("Garbage collection", r"(?i)garbage|deleting|freed|removing old"),
            ("Signing in", r"(?i)\bauth|token|login|credential"),
        ],
    },
    Known {
        name: "NixOS",
        sources: &[
            "nixos",
            "switch-to-configuration",
            "nixos-rebuild",
            "nixos-upgrade",
            "sekka-update",
            "yukimi-helper",
        ],
        topics: &[
            ("System changes", r"(?i)switching|activat|configuration|generation"),
            ("Units changed", r"(?i)(restarting|reloading|stopping|starting) the following units|\bunits\b"),
            ("Updates", r"(?i)update|upgrade|nightly|channel|flake"),
        ],
    },
    Known {
        name: "Aizome",
        sources: &["aizome", "aizome-*", "aizome-session"],
        topics: &[
            ("Panics", r"panicked at|RUST_BACKTRACE|stack backtrace"),
            ("Displays", r"(?i)\boutput\b|display|monitor|\bmode\b|hotplug|connector|edid|scale"),
            ("Input", r"(?i)keyboard|pointer|touchpad|libinput|\bseat\b|keymap|ibus|text.input"),
            ("Drawing", r"(?i)frame|render|\bEGL\b|\bGBM\b|\bDRM\b|vblank|page.flip|swapchain|shader"),
            ("Portals and sharing", r"(?i)portal|screencast|screen ?share|pipewire"),
            ("Locking and sleep", r"(?i)\block|unlock|idle|suspend|resume|sleep"),
        ],
    },
    Known {
        name: "the login screen's session",
        sources: &["xwayland", "xwayland-satellite", "xorg", "x"],
        topics: &[
            ("X clients", r"(?i)client|window|xcb|selection"),
            ("Graphics", r"(?i)\bglx|\begl|\bdri|glamor|\bgbm|driver"),
        ],
    },
    Known {
        name: "storage",
        sources: &["udisksd", "systemd-fsck", "fsck", "smartd", "systemd-cryptsetup", "lvm", "mdadm", "btrfs", "zed"],
        topics: &[
            ("Mounts", r"(?i)\bmount|unmount|\bmounted"),
            ("Encryption", r"(?i)luks|unlock|crypt|passphrase"),
            ("Disk health", r"(?i)smart|health|sector|reallocat|pending|temperature|error"),
            ("File system checks", r"(?i)fsck|clean|check|repair|orphan"),
        ],
    },
    Known {
        name: "power",
        sources: &["upowerd", "power-profiles-daemon", "thermald", "tlp", "auto-cpufreq", "powertop"],
        topics: &[
            ("Battery", r"(?i)battery|charg|\bAC\b|discharg"),
            ("Power profiles", r"(?i)profile|performance|balanced|power-saver"),
            ("Temperature", r"(?i)thermal|temperature|throttl"),
        ],
    },
    Known {
        name: "devices",
        sources: &["systemd-udevd", "udevadm", "fwupd", "fwupdmgr", "bolt", "boltd"],
        topics: &[
            ("Device rules", r"(?i)rule|uevent|\bdevice\b|udev"),
            ("Firmware updates", r"(?i)firmware|update|\blvfs|capsule"),
            ("Thunderbolt", r"(?i)thunderbolt|\bbolt\b|usb4|authoriz"),
        ],
    },
    Known {
        name: "name resolution and time",
        sources: &["systemd-resolved", "dnsmasq", "unbound", "avahi-daemon", "systemd-timesyncd", "chronyd", "ntpd"],
        topics: &[
            ("DNS", r"(?i)\bdns|dnssec|server|degraded feature|query"),
            ("Local network names", r"(?i)mdns|avahi|\.local\b|registering|withdrawing"),
            ("Time sync", r"(?i)contacted|synchroni|time server|clock|\bntp|offset|leap"),
        ],
    },
    Known {
        name: "crashes",
        sources: &["systemd-coredump", "coredumpctl", "abrt*", "drkonqi"],
        topics: &[("Core dumps", r"dumped core"), ("Stack traces", r"Stack trace of thread|#\d+ +0x")],
    },
    Known {
        name: "printing",
        sources: &["cupsd", "cups-browsed", "cups-pk-helper-mechanism", "ippusbxd", "ipp-usb", "saned"],
        topics: &[
            ("Print jobs", r"(?i)\bjob\b|printing|print job|queued|completed"),
            ("Printers found", r"(?i)printer|queue|added|discover|bonjour|dnssd"),
        ],
    },
    Known {
        name: "the browser",
        sources: &[
            "firefox",
            "firefox-bin",
            "librewolf",
            "chromium",
            "chromium-browser",
            "chrome",
            "google-chrome",
            "google-chrome-stable",
            "brave",
            "brave-browser",
            "vivaldi",
            "electron",
            "code",
            "codium",
            "slack",
            "discord",
            "obsidian",
            "signal-desktop",
        ],
        topics: &[
            ("Graphics", r"(?i)\bgpu\b|vulkan|\begl\b|\bgl\b|va-?api|viz|angle|dmabuf|gbm|mesa"),
            ("Crashes and the sandbox", r"(?i)crash|sandbox|sigsegv|segfault|minidump|zygote"),
            ("Audio and video", r"(?i)audio|pulse|pipewire|media|codec|ffmpeg|webrtc"),
            ("Network", r"(?i)net::|networkservice|err_[a-z_]+|ssl|tls|certificate|dns"),
            ("Extensions and scripts", r"(?i)extension|addon|javascript|console\.|webextension"),
        ],
    },
    Known {
        name: "Qt and GTK apps",
        sources: &[
            "telegram",
            "telegram-desktop",
            "kdeconnectd",
            "plasmashell",
            "kwin_wayland",
            "nautilus",
            "gnome-shell",
            "evolution",
            "geary",
            "gimp",
            "inkscape",
            "krita",
            "steam",
            "lutris",
        ],
        topics: &[
            ("Wayland", r"(?i)wayland|wl_|xdg_|surface|text.?input"),
            ("Toolkit warnings", r"(?i)\bqt\.|\bQ[A-Z][a-z]+|Gtk-|Gdk-|GLib-|GLib-GIO|Gtk:|Gdk:|critical"),
            ("Media", r"(?i)audio|video|codec|ffmpeg|mp3|gstreamer|pulse|pipewire"),
            ("D-Bus", r"(?i)d-?bus|org\.freedesktop|portal"),
        ],
    },
];

/// Each program's topics' patterns, compiled once.
static COMPILED: LazyLock<Vec<Vec<Arc<Regex>>>> = LazyLock::new(|| {
    KNOWN
        .iter()
        .map(|k| k.topics.iter().map(|(_, p)| Arc::new(Regex::new(p).expect("a known topic's pattern"))).collect())
        .collect()
});

fn source_is(source: &str, pattern: &str) -> bool {
    match pattern.strip_suffix('*') {
        Some(prefix) => source.starts_with(prefix),
        None => source == pattern,
    }
}

/// What Nisshi knows of the program that wrote `e`, and its topics'
/// patterns.
pub fn of(e: &Entry) -> Option<(&'static Known, &'static [Arc<Regex>])> {
    let source = e.source.to_lowercase();
    let stem = crate::search::unit_stem(e.unit.as_deref()).map(str::to_lowercase);
    let at = KNOWN.iter().position(|k| {
        k.sources.iter().any(|p| source_is(&source, p) || stem.as_deref().is_some_and(|s| source_is(s, p)))
            || (e.transport == Transport::Kernel && k.sources.contains(&"kernel"))
    })?;
    Some((&KNOWN[at], &COMPILED[at]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_pattern_compiles() {
        assert_eq!(COMPILED.len(), KNOWN.len());
        for (k, c) in KNOWN.iter().zip(COMPILED.iter()) {
            assert_eq!(k.topics.len(), c.len(), "{}", k.name);
            for s in k.sources {
                assert_eq!(*s, s.to_lowercase(), "{}'s sources are lower case", k.name);
            }
        }
    }

    #[test]
    fn topics_find_what_they_say() {
        let at = |name: &str| KNOWN.iter().position(|k| k.name == name).unwrap();
        let topic = |k: usize, t: &str| {
            KNOWN[k].topics.iter().position(|(n, _)| *n == t).map(|i| COMPILED[k][i].clone()).unwrap()
        };
        let nm = at("NetworkManager");
        assert!(topic(nm, "DHCP").is_match("dhcp4 (wlp14s0): state changed new lease, address=192.168.1.23"));
        assert!(topic(nm, "Wi-Fi").is_match("device (wlp14s0): supplicant interface state: completed"));
        let ssh = at("SSH");
        assert!(
            topic(ssh, "Failed logins").is_match("Failed password for invalid user admin from 10.0.0.5 port 4242 ssh2")
        );
        assert!(!topic(ssh, "Failed logins").is_match("Accepted publickey for alice"));
        let kernel = at("the kernel");
        assert!(topic(kernel, "Out of memory").is_match("Out of memory: Killed process 4321 (firefox)"));
        assert!(topic(kernel, "Crashes and traps").is_match("traps: gamescope[1234] trap int3 ip:55"));
    }
}
