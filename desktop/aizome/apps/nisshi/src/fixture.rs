//! A made-up boot of a made-up desktop, for tests and pictures: no message
//! on this computer is shown. Five hours and ten minutes of a day: the
//! kernel starting, systemd, Wi-Fi joining, a Qt app that won't stop
//! warning, a burst of SSH probes, a unit that fails, a program that
//! crashes, and an administrator at work.

use std::sync::Arc;

use crate::entry::{Cursor, Entry, Interner, Priority, Transport};
use crate::filter::{COREDUMP, UNIT_FAILED};
use crate::protocol::{BootInfo, FileInfo};
use crate::time::Clock;

/// Thursday 29 October 2026, 16:05:00 in Chicago.
pub const NOW: i64 = 1_793_307_900_000_000;
/// When the boot began: 10:55:00.
pub const BOOTED: i64 = NOW - (5 * 3600 + 10 * 60) * 1_000_000;
pub const BOOT_ID: &str = "573e76de00ca44adbdcb30ca43863c22";
pub const UID: u32 = 1000;

pub fn clock() -> Clock {
    Clock::fixed("America/Chicago", NOW)
}

/// A small, repeatable source of numbers.
struct Dice(u64);

impl Dice {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

struct Writer {
    intern: Interner,
    out: Vec<Entry>,
    seqnum: u64,
}

/// What's said about a message beyond its words.
#[derive(Clone, Copy)]
struct Who {
    source: &'static str,
    unit: Option<&'static str>,
    pid: u32,
    uid: Option<u32>,
    transport: Transport,
    facility: Option<u8>,
}

const KERNEL: Who =
    Who { source: "kernel", unit: None, pid: 0, uid: None, transport: Transport::Kernel, facility: Some(0) };

const fn sys(source: &'static str, unit: &'static str, pid: u32) -> Who {
    Who { source, unit: Some(unit), pid, uid: Some(0), transport: Transport::Journal, facility: Some(3) }
}

const fn mine(source: &'static str, unit: &'static str, pid: u32) -> Who {
    Who { source, unit: Some(unit), pid, uid: Some(UID), transport: Transport::Stdout, facility: None }
}

impl Writer {
    fn say(&mut self, at: i64, who: Who, p: Priority, message: &str) -> &mut Entry {
        self.say_about(at, who, p, message, None, None)
    }

    /// A line of an app the desktop launched, under that launch's own
    /// identifier, in the app's scope ([`crate::launch`]).
    fn launched(&mut self, at: i64, launch: &str, line: &str) {
        let koe = mine("Koe", r"app-aizome-org.aizome.Koe-5c1e0a9b3d2f4e67.scope", 5120);
        let id = self.intern.get(launch);
        self.say(at, koe, Priority::Info, line).launch = Some(id);
    }

    fn say_about(
        &mut self,
        at: i64,
        who: Who,
        p: Priority,
        message: &str,
        about: Option<&str>,
        id: Option<&str>,
    ) -> &mut Entry {
        self.seqnum += 1;
        let mono = (at - BOOTED).max(0) as u64;
        let e = Entry {
            time: at,
            mono,
            priority: p,
            transport: who.transport,
            facility: who.facility,
            source: self.intern.get(who.source),
            unit: who.unit.map(|u| self.intern.get(u)),
            about: about.map(|u| self.intern.get(u)),
            pid: who.pid,
            uid: who.uid,
            shape: crate::shape::fingerprint(message),
            message: message.into(),
            message_id: id.map(|i| self.intern.get(i)),
            boot: Some(self.intern.get(BOOT_ID)),
            cursor: Cursor::Parts {
                seqnum_id: self.intern.get("9f3a82e3f878453f8db379ec9f7c7a99"),
                seqnum: 0xcc000 + self.seqnum,
                boot: self.intern.get(BOOT_ID),
                mono,
                realtime: at as u64,
                xor: self.seqnum.wrapping_mul(0x9e37_79b9_7f4a_7c15),
            },
            seq: 0,
            flags: 0,
            styles: None,
            launch: None,
        }
        .finish();
        self.out.push(e);
        self.out.last_mut().unwrap()
    }
}

const S: i64 = 1_000_000;
const MIN: i64 = 60 * S;

/// The boot's messages, oldest first.
pub fn journal() -> Vec<Entry> {
    let mut w = Writer { intern: Interner::default(), out: Vec::new(), seqnum: 0 };
    let mut d = Dice(0x5eed_1234_abcd_0001);
    let t0 = BOOTED;
    use Priority::*;
    // The kernel starts.
    let boot_lines: [(Priority, &str); 22] = [
        (
            Notice,
            "Linux version 6.17.3 (nixbld@localhost) (gcc (GCC) 14.3.0, GNU ld (GNU Binutils) 2.44) #1-NixOS SMP PREEMPT_DYNAMIC",
        ),
        (Info, "Command line: initrd=\\EFI\\nixos\\initrd.efi init=/nix/store/system/init loglevel=4"),
        (Info, "BIOS-provided physical RAM map:"),
        (Info, "DMI: Micro-Star International Co., Ltd. MS-7E49/MAG X870 TOMAHAWK WIFI, BIOS 1.A61 08/21/2026"),
        (Info, "ACPI: RSDP 0x00000000A5B1E014 000024 (v02 ALASKA)"),
        (Info, "smpboot: CPU0: AMD Ryzen 9 9950X3D 16-Core Processor (family: 0x1a, model: 0x44, stepping: 0x0)"),
        (
            Warning,
            "ACPI BIOS Error (bug): Could not resolve symbol [\\_SB.PCI0.GPP7.UP00.DP40.UP00.DP68], AE_NOT_FOUND",
        ),
        (Info, "usb 1-2: new high-speed USB device number 3 using xhci_hcd"),
        (Info, "usb 1-2: New USB device found, idVendor=046d, idProduct=c548, bcdDevice= 5.01"),
        (Info, "usb 3-4: new full-speed USB device number 2 using xhci_hcd"),
        (Info, "nvme nvme1: 32/0/0 default/read/poll queues"),
        (
            Info,
            "EXT4-fs (nvme1n1p2): mounted filesystem 3c7a9a4e-2f1b-4c0e-9d77-1f0e8a3b2c11 ro with ordered data mode.",
        ),
        (Info, "amdgpu 0000:7c:00.0: amdgpu: SMU is initialized successfully!"),
        (Warning, "nvidia: loading out-of-tree module taints kernel."),
        (Info, "[drm] Initialized nvidia-drm 0.0.0 for 0000:01:00.0 on minor 1"),
        (Info, "Bluetooth: Core ver 2.22"),
        (Info, "iwlwifi 0000:0e:00.0: Detected Intel(R) Wi-Fi 7 BE200 320MHz, REV=0x472"),
        (Info, "wlp14s0: renamed from wlan0"),
        (Info, "EXT4-fs (nvme1n1p2): re-mounted 3c7a9a4e-2f1b-4c0e-9d77-1f0e8a3b2c11 r/w."),
        (Info, "Adding 33554428k swap on /dev/nvme1n1p3.  Priority:-2 extents:1 across:33554428k SS"),
        (Info, "r8169 0000:0d:00.0 enp13s0: Link is Down"),
        (Info, "audit: type=1400 audit(1793289302.112:2): apparmor=\"STATUS\" operation=\"profile_load\""),
    ];
    for (i, (p, m)) in boot_lines.iter().enumerate() {
        w.say(t0 + i as i64 * 41_000, KERNEL, *p, m);
    }
    let pid1 = sys("systemd", "init.scope", 1);
    let starts = [
        "Started Journal Service.",
        "Started Rule-based Manager for Device Events and Files.",
        "Reached target Local File Systems.",
        "Started D-Bus System Message Bus.",
        "Started Network Manager.",
        "Started Login Service.",
        "Started Disk Manager.",
        "Started Bluetooth service.",
        "Started Daemon for power management.",
        "Reached target Multi-User System.",
        "Started greetd.",
        "Reached target Graphical Interface.",
    ];
    for (i, m) in starts.iter().enumerate() {
        w.say(t0 + 1_200_000 + i as i64 * 210_000, pid1, Info, m);
    }
    w.say(
        t0 + 4 * S,
        pid1,
        Info,
        "Startup finished in 4.512s (firmware) + 2.103s (loader) + 1.882s (kernel) + 6.120s (userspace) = 14.617s.",
    );
    // Wi-Fi joins.
    let nm = sys("NetworkManager", "NetworkManager.service", 1191);
    let wpa = sys("wpa_supplicant", "wpa_supplicant.service", 1203);
    let join = |w: &mut Writer, at: i64| {
        w.say(at, nm, Info, "<info>  [1793289303.4957] device (wlp14s0): state change: disconnected -> prepare (reason 'none', managed-type: 'full')");
        w.say(at + 30_000, nm, Info, "<info>  [1793289303.5012] device (wlp14s0): Activation: starting connection 'Kotatsu' (6b1e0c2a-1d4f-4b8e-9a6c-2f7d3e1b0a99)");
        w.say(
            at + 90_000,
            wpa,
            Info,
            "wlp14s0: SME: Trying to authenticate with 74:83:c2:4a:91:0e (SSID='Kotatsu' freq=5745 MHz)",
        );
        w.say(
            at + 140_000,
            wpa,
            Info,
            "wlp14s0: Trying to associate with 74:83:c2:4a:91:0e (SSID='Kotatsu' freq=5745 MHz)",
        );
        w.say(
            at + 190_000,
            wpa,
            Info,
            "wlp14s0: CTRL-EVENT-CONNECTED - Connection to 74:83:c2:4a:91:0e completed [id=0 id_str=]",
        );
        w.say(
            at + 230_000,
            nm,
            Info,
            "<info>  [1793289303.7233] device (wlp14s0): supplicant interface state: associating -> completed",
        );
        w.say(
            at + 260_000,
            nm,
            Info,
            "<info>  [1793289303.7710] dhcp4 (wlp14s0): state changed new lease, address=192.168.1.23",
        );
        w.say(at + 300_000, nm, Info, "<info>  [1793289303.8001] device (wlp14s0): state change: ip-config -> activated (reason 'none', managed-type: 'full')");
    };
    join(&mut w, t0 + 6 * S);
    w.say(t0 + 6 * S + 400_000, nm, Warning, "<warn>  [1793289315.4957] device (p2p-dev-wlp14s0): error setting IPv4 forwarding to '0': Resource temporarily unavailable");
    // A login, and the desktop starting.
    let logind = sys("systemd-logind", "systemd-logind.service", 1180);
    w.say(t0 + 20 * S, logind, Info, "New session 2 of user alice.");
    let session = mine("aizome-session", "aizome-session.service", 2210);
    let shell = mine("aizome-shell", "aizome-shell.service", 2241);
    let comp = mine("aizome", "aizome.service", 2230);
    w.say(t0 + 21 * S, session, Info, "Aizome 0.1.0 starting on seat0");
    w.say(t0 + 21 * S + 300_000, comp, Info, "output DP-2: 3840x2160@143.99 Hz, scale 2");
    w.say(t0 + 21 * S + 320_000, comp, Info, "output HDMI-A-1: 2560x1440@59.95 Hz, scale 1");
    w.say(t0 + 21 * S + 600_000, shell, Info, "polkit agent registered");
    let pw = mine("pipewire", "pipewire.service", 2273);
    let wp = mine("wireplumber", "wireplumber.service", 2280);
    w.say(t0 + 22 * S, wp, Info, "spa.bluez5: BlueZ system service is available");
    // The rest of the day.
    let telegram = mine("Telegram", "app-org.telegram.desktop-4142.scope", 4142);
    let firefox = mine("firefox", "app-firefox-3310.scope", 3310);
    let nix = sys("nix-daemon", "nix-daemon.service", 7571);
    let sshd = Who { facility: Some(10), ..sys("sshd", "sshd.service", 1301) };
    let sudo = Who { facility: Some(10), ..sys("sudo", "session-2.scope", 6754) };
    let mut t = t0 + 60 * S;
    let end = NOW - 2 * S;
    let mut tick = 0u64;
    while t < end {
        tick += 1;
        match d.below(20) {
            0..=6 => {
                let m = "virtual void QtWaylandClient::QWaylandTextInputv3::enableSurface(wl_surface*) Trying to enable 0x5c33391b8350 but 0x5c33391b8350 is already enabled";
                w.say(t, telegram, Warning, m);
            }
            7 => {
                w.say(t, firefox, Warning, "[GFX1-]: Failed to create EGLSurface: 0x3003");
            }
            8 => {
                let m = format!(
                    "[Parent 3310, IPC I/O Parent] WARNING: process {} exited on signal 9: file /build/firefox/ipc/chromium/src/base/process_util_posix.cc:264",
                    3400 + d.below(900)
                );
                w.say(t, firefox, Info, &m);
            }
            9 => {
                let m =
                    format!("spa.alsa: hw:2: snd_pcm_avail after recover: Broken pipe (after {} ms)", 3 + d.below(30));
                w.say(t, pw, Warning, &m);
            }
            10 => {
                let n = d.below(40) + 1;
                w.say(t, nix, Info, &format!("accepted connection from pid {}, user alice (trusted)", 9000 + n));
                w.say(
                    t + 400_000,
                    nix,
                    Info,
                    &format!(
                        "building '/nix/store/{}-sekka-system-26.11.drv'...",
                        ["0bpaf9k8", "1xw2c3vr", "7q4hsm2d"][(n % 3) as usize]
                    ),
                );
            }
            11 => {
                w.say(
                    t,
                    nm,
                    Info,
                    &format!(
                        "<info>  [{}.{:04}] dhcp4 (wlp14s0): state changed new lease, address=192.168.1.23",
                        t / S,
                        d.below(10000)
                    ),
                );
            }
            12 => {
                w.say(
                    t,
                    wpa,
                    Info,
                    &format!(
                        "wlp14s0: CTRL-EVENT-SIGNAL-CHANGE above=1 signal=-{} noise=9999 txrate=2882000",
                        40 + d.below(20)
                    ),
                );
            }
            13 => {
                w.say(t, shell, Info, &format!("notifications: {} waiting", d.below(4)));
            }
            14 => {
                w.say(
                    t,
                    comp,
                    Debug,
                    &format!(
                        "frame budget: p50 {}.{} ms, p95 {}.{} ms",
                        1 + d.below(2),
                        d.below(10),
                        2 + d.below(3),
                        d.below(10)
                    ),
                );
            }
            15 => {
                w.say(t, logind, Info, "Watching system buttons on /dev/input/event4 (Power Button)");
            }
            _ => {
                w.say(t, pid1, Info, "Started AI background workload.");
                w.say(t + 50_000, pid1, Info, "ai-workload.service: Deactivated successfully.");
            }
        }
        t += (20 + d.below(70) as i64) * S;
        let _ = tick;
    }
    // Probes at SSH, at 14:32.
    let burst = NOW - (93 * MIN);
    for i in 0..34 {
        let at = burst + i * 1_700_000;
        let ip = format!("203.0.113.{}", 7 + (i % 5));
        w.say(at, sshd, Info, &format!("Invalid user admin from {ip} port {}", 40000 + i * 37));
        w.say(
            at + 200_000,
            sshd,
            Error,
            &format!("error: kex_exchange_identification: Connection closed by remote host from {ip}"),
        );
    }
    w.say(
        burst - 4 * MIN,
        sshd,
        Info,
        "Accepted publickey for alice from 192.168.1.40 port 51334 ssh2: ED25519 SHA256:q2tW8m",
    );
    // An administrator at work.
    let at = NOW - 70 * MIN;
    w.say(
        at,
        sudo,
        Notice,
        "alice : TTY=pts/1 ; PWD=/home/alice ; USER=root ; COMMAND=/run/current-system/sw/bin/nixos-rebuild switch",
    );
    w.say(at + 20_000, sudo, Info, "pam_unix(sudo:session): session opened for user root(uid=0) by alice(uid=1000)");
    w.say(at + 30 * S, sudo, Info, "pam_unix(sudo:session): session closed for user root");
    w.say(NOW - 69 * MIN, Who { facility: Some(10), ..sys("sudo", "session-2.scope", 6799) }, Error, "alice : 3 incorrect password attempts ; TTY=pts/1 ; PWD=/home/alice ; USER=root ; COMMAND=/run/current-system/sw/bin/true");
    // A unit that fails.
    let at = NOW - 48 * MIN;
    let fwupd = sys("fwupdmgr", "fwupd-refresh.service", 8122);
    w.say(at, pid1, Info, "Starting Refresh fwupd metadata and update motd...");
    w.say(at + 900_000, fwupd, Error, "Failed to update metadata for lvfs: Failed to download https://cdn.fwupd.org/downloads/firmware.xml.zst: Could not resolve host");
    w.say_about(
        at + 950_000,
        pid1,
        Notice,
        "fwupd-refresh.service: Main process exited, code=exited, status=1/FAILURE",
        Some("fwupd-refresh.service"),
        None,
    );
    w.say_about(
        at + 960_000,
        pid1,
        Warning,
        "fwupd-refresh.service: Failed with result 'exit-code'.",
        Some("fwupd-refresh.service"),
        Some(UNIT_FAILED),
    );
    w.say_about(
        at + 970_000,
        pid1,
        Error,
        "Failed to start Refresh fwupd metadata and update motd.",
        Some("fwupd-refresh.service"),
        None,
    );
    // Koe, launched from the Toolchest twice: once it starts, once its
    // microphone has gone.
    let first = "aizome-launch-0a1b2c3d4e5f60718293a4b5c6d7e8f9";
    let at = NOW - 170 * MIN;
    w.launched(at, first, "koe: loading ggml-base.en (142 MB)");
    w.launched(at + 1_200_000, first, "koe: model ready in 1.2 s, using Vulkan on Radeon Graphics");
    w.launched(at + 1_400_000, first, "koe: listening on alsa_input.usb-Blue_Microphones_Yeti-00.analog-stereo");
    let second = "aizome-launch-f332d86ed3fac3deb4589d0998c3a08e";
    let at = NOW - 31 * MIN;
    w.launched(at, second, "koe: loading ggml-base.en (142 MB)");
    w.launched(at + 1_100_000, second, "ERROR: no such source: alsa_input.usb-Blue_Microphones_Yeti-00.analog-stereo");
    w.launched(at + 1_150_000, second, "koe: daemon did not become ready");
    // A program that crashes.
    let at = NOW - 26 * MIN;
    w.say(at, KERNEL, Info, "traps: gamescope-wl[298773] trap int3 ip:55d03a2b1c4e sp:7ffd5f1e2a90 error:0 in gamescope-wl[2b1c4e,55d03a200000+1a2000]");
    let dump = "Process 298773 (gamescope-wl) of user 1000 dumped core.\n\nModule libvulkan_radeon.so without build-id.\nStack trace of thread 298773:\n#0  0x00007f3c2a6a2e9c __pthread_kill_implementation (libc.so.6 + 0x92e9c)\n#1  0x00007f3c2a64a0e6 raise (libc.so.6 + 0x3a0e6)\n#2  0x000055d03a2b1c4e _ZN9gamescope12CVulkanTexture4InitEv (gamescope-wl + 0xb1c4e)\n#3  0x000055d03a2a90d1 main (gamescope-wl + 0xa90d1)\nELF object binary architecture: AMD x86-64";
    let e = w.say_about(
        at + 1_400_000,
        sys("systemd-coredump", "systemd-coredump@3-298790-0.service", 298790),
        Critical,
        dump,
        None,
        Some(COREDUMP),
    );
    e.uid = Some(0);
    // The last minute: what the window opens on.
    let recent: [(Who, Priority, &str); 14] = [
        (
            nm,
            Info,
            "<info>  [1793307842.1102] device (wlp14s0): set-hw-addr: set MAC address to 3a:91:7c:0e:44:1b (scanning)",
        ),
        (wpa, Info, "wlp14s0: CTRL-EVENT-SIGNAL-CHANGE above=0 signal=-71 noise=9999 txrate=6000"),
        (wpa, Warning, "wlp14s0: CTRL-EVENT-BEACON-LOSS"),
        (wpa, Info, "wlp14s0: CTRL-EVENT-DISCONNECTED bssid=74:83:c2:4a:91:0e reason=4 locally_generated=1"),
        (nm, Warning, "<warn>  [1793307851.0021] device (wlp14s0): link timed out."),
        (
            nm,
            Info,
            "<info>  [1793307851.0034] device (wlp14s0): state change: activated -> failed (reason 'ssid-not-found', managed-type: 'full')",
        ),
        (
            nm,
            Info,
            "<info>  [1793307853.2210] device (wlp14s0): Activation: starting connection 'Kotatsu' (6b1e0c2a-1d4f-4b8e-9a6c-2f7d3e1b0a99)",
        ),
        (wpa, Info, "wlp14s0: CTRL-EVENT-CONNECTED - Connection to 74:83:c2:4a:91:0e completed [id=0 id_str=]"),
        (nm, Info, "<info>  [1793307854.0102] dhcp4 (wlp14s0): state changed new lease, address=192.168.1.23"),
        (shell, Info, "network: Kotatsu connected again after 3 s"),
        // The compositor's own, through `tracing` with its colours, which
        // journald files at Info whatever the line says.
        (
            comp,
            Info,
            "\u{1b}[2m2026-10-29T21:04:44.120311Z\u{1b}[0m \u{1b}[32m INFO\u{1b}[0m \u{1b}[2maizome_comp_core::shell\u{1b}[0m\u{1b}[2m:\u{1b}[0m window opened \u{1b}[3mid\u{1b}[0m\u{1b}[2m=\u{1b}[0m41 \u{1b}[3mapp_id\u{1b}[0m\u{1b}[2m=\u{1b}[0m\"org.aizome.Hako\"",
        ),
        (
            comp,
            Info,
            "\u{1b}[2m2026-10-29T21:04:45.902517Z\u{1b}[0m \u{1b}[33m WARN\u{1b}[0m \u{1b}[2maizome_comp_logic::outputs\u{1b}[0m\u{1b}[2m:\u{1b}[0m HDMI-A-1: no EDID, assuming 60 Hz",
        ),
        (
            telegram,
            Warning,
            "virtual void QtWaylandClient::QWaylandTextInputv3::enableSurface(wl_surface*) Trying to enable 0x5c33391b8350 but 0x5c33391b8350 is already enabled",
        ),
        (pid1, Info, "Started AI background workload."),
    ];
    for (i, (who, p, m)) in recent.into_iter().enumerate() {
        w.say(NOW - 60 * S + i as i64 * 4_200_000, who, p, m);
    }
    let mut out = w.out;
    out.sort_by_key(|e| e.time);
    out
}

/// The journal's boots: this one and three before.
pub fn boots() -> Vec<BootInfo> {
    let day = 86_400 * S;
    vec![
        BootInfo {
            index: -3,
            boot_id: "9430a76c4cfb47faad6f618b6573b291".into(),
            first_entry: BOOTED - 2 * day,
            last_entry: BOOTED - 2 * day + 17 * 3600 * S,
        },
        BootInfo {
            index: -2,
            boot_id: "1b4755f5676f4d01ae12881fac93a1ef".into(),
            first_entry: BOOTED - day,
            last_entry: BOOTED - day + 22 * 60 * S,
        },
        BootInfo {
            index: -1,
            boot_id: "c7d1ac8e55d840b98bda6795747f4ef6".into(),
            first_entry: BOOTED - day + 23 * 60 * S,
            last_entry: BOOTED - 15 * 3600 * S,
        },
        BootInfo { index: 0, boot_id: BOOT_ID.into(), first_entry: BOOTED, last_entry: NOW },
    ]
}

/// /var/log's text logs.
pub fn files() -> Vec<FileInfo> {
    vec![
        FileInfo { path: "/var/log/boot.log".into(), size: 39_722, modified: BOOTED + 30 * S, readable: false },
        FileInfo { path: "/var/log/Xorg.0.log".into(), size: 41_220, modified: BOOTED + 25 * S, readable: true },
        FileInfo {
            path: "/var/log/calamares-nixos/session.log".into(),
            size: 188_301,
            modified: BOOTED - 14 * 86_400 * S,
            readable: true,
        },
    ]
}

/// The lines of /var/log/Xorg.0.log, as entries.
pub fn xorg() -> Vec<Entry> {
    let source: Arc<str> = "Xorg.0.log".into();
    let c = clock();
    [
        "[    38.201] (--) Log file renamed from \"/var/log/Xorg.pid-2301.log\" to \"/var/log/Xorg.0.log\"",
        "[    38.204] X.Org X Server 1.21.1.18",
        "[    38.211] (==) Using config directory: \"/etc/X11/xorg.conf.d\"",
        "[    38.311] (II) Loading /nix/store/xorg-server/lib/xorg/modules/drivers/modesetting_drv.so",
        "[    38.402] (EE) Failed to load module \"nv\" (module does not exist, 0)",
        "[    38.410] (WW) Falling back to old probe method for modesetting",
        "[    38.590] (II) modeset(0): Output DP-2 connected",
    ]
    .iter()
    .map(|l| crate::entry::from_line(l, &source, 0, &c))
    .collect()
}

/// An entry as `journalctl -o json` writes it (for tests that give the
/// reader a stand-in journalctl).
pub fn json(e: &Entry) -> String {
    use serde_json::{Map, Value};
    let mut m = Map::new();
    let mut put = |k: &str, v: String| {
        m.insert(k.to_owned(), Value::String(v));
    };
    if let Some(c) = e.cursor.text() {
        put("__CURSOR", c);
    }
    put("__REALTIME_TIMESTAMP", e.time.to_string());
    put("__MONOTONIC_TIMESTAMP", e.mono.to_string());
    if let Some(b) = &e.boot {
        put("_BOOT_ID", b.to_string());
    }
    // With its colours as the program wrote them: bytes, as journald writes
    // a message with control characters.
    let styled = e.styles.as_deref().map(|runs| crate::ansi::encode(&e.message, runs));
    if styled.is_none() {
        put("MESSAGE", e.message.to_string());
    }
    // The level journald filed it at: a level read from the text wasn't.
    let filed = if e.flags & crate::entry::WRITTEN_LEVEL != 0 { Priority::Info } else { e.priority };
    put("PRIORITY", filed.level().to_string());
    // A launch's own identifier, as the shell gives a launch it watches.
    put("SYSLOG_IDENTIFIER", e.launch.as_deref().unwrap_or(&e.source).to_string());
    if let Some(f) = e.facility {
        put("SYSLOG_FACILITY", f.to_string());
    }
    if e.pid > 0 {
        put("_PID", e.pid.to_string());
    }
    if let Some(u) = e.uid {
        put("_UID", u.to_string());
    }
    let transport = match e.transport {
        Transport::Kernel => "kernel",
        Transport::Stdout => "stdout",
        Transport::Syslog => "syslog",
        Transport::Audit => "audit",
        Transport::Driver => "driver",
        _ => "journal",
    };
    put("_TRANSPORT", transport.into());
    if let Some(u) = &e.unit {
        if e.uid == Some(UID) {
            put("_SYSTEMD_UNIT", format!("user@{UID}.service"));
            put("_SYSTEMD_USER_UNIT", u.to_string());
        } else {
            put("_SYSTEMD_UNIT", u.to_string());
        }
    }
    if let Some(u) = &e.about {
        put("UNIT", u.to_string());
    }
    if let Some(id) = &e.message_id {
        put("MESSAGE_ID", id.to_string());
    }
    if let Some(raw) = styled {
        m.insert("MESSAGE".into(), Value::Array(raw.bytes().map(Value::from).collect()));
    }
    Value::Object(m).to_string()
}

/// A large boot: the made-up one, `times` over, one after another (the
/// last ending now).
pub fn large(times: usize) -> Vec<Entry> {
    let one = journal();
    let span = NOW - BOOTED + 1_000_000;
    let mut out = Vec::with_capacity(one.len() * times);
    for k in (0..times as i64).rev() {
        out.extend(one.iter().map(|e| Entry { time: e.time - k * span, ..e.clone() }));
    }
    out
}

/// Make `dir` a stand-in for this computer's journal and /var/log: the
/// made-up boot as journalctl's JSON, a `journalctl` that answers the
/// reader's requests from it (the newest three come as it follows), the
/// boots, and a log folder with one text log. Point the reader at it with
/// `NISSHI_JOURNALCTL=dir/journalctl` and `NISSHI_LOG_DIR=dir/log` (both
/// taken only when the reader doesn't run as root).
pub fn stage(dir: &std::path::Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::create_dir_all(dir.join("log/sub"))?;
    let all: Vec<String> = journal().iter().map(json).collect();
    let (old, new) = all.split_at(all.len() - 3);
    std::fs::write(dir.join("entries.jsonl"), old.join("\n") + "\n")?;
    std::fs::write(dir.join("follow.jsonl"), new.join("\n") + "\n")?;
    std::fs::write(dir.join("boots.json"), serde_json::to_string(&boots()).unwrap_or_default())?;
    std::fs::write(
        dir.join("log/sub/app.log"),
        "2026-10-29T16:00:00-05:00 starting\n2026-10-29T16:00:01-05:00 ERROR: no disk\n",
    )?;
    std::fs::write(dir.join("log/wtmp"), [0u8, 1, 2])?;
    let path = std::env::var("PATH").unwrap_or_default();
    let d = dir.display();
    let script = format!(
        r#"#!/bin/sh
export PATH='{path}'
echo "$*" >> '{d}/asked'
case " $* " in
  *" --list-boots "*) cat '{d}/boots.json'; exit 0 ;;
esac
cursor=''
lines=''
for a in "$@"; do
  case "$a" in
    --cursor=*) cursor="${{a#--cursor=}}" ;;
    --lines=*) lines="${{a#--lines=}}" ;;
  esac
done
case " $* " in
  *" --follow "*) cat '{d}/follow.jsonl'; echo $$ > '{d}/follow.pid'; exec sleep 3600 ;;
esac
if [ -n "$cursor" ] && [ "$lines" = 1 ]; then
  grep -F "\"__CURSOR\":\"$cursor\"" '{d}/entries.jsonl' '{d}/follow.jsonl' -h | head -n 1
  exit 0
fi
if [ -n "$cursor" ]; then exit 0; fi
tac '{d}/entries.jsonl' | head -n "${{lines:-1000000}}"
"#
    );
    let journalctl = dir.join("journalctl");
    std::fs::write(&journalctl, script)?;
    std::fs::set_permissions(&journalctl, std::fs::Permissions::from_mode(0o755))
}
