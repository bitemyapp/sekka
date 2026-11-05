//! Made-up machines, conversations and notes for the window's fixtures
//! (`aizome-tasuke --fixture NAME`), its pictures and its tests. Nothing
//! here comes from a real computer or person. The model catalog is the
//! real one: on these machines, the Models page shows what `tasuke-models`
//! would recommend.
//!
//! The conversations follow Tasuke's skills (`apps/tasuke/skills`): the
//! commands are the ones they give, and the outputs are what those commands
//! print, with made-up numbers.

use serde_json::json;
use tasuke_models::{Bytes, Cpu, Gpu, GpuMemory, GpuVendor, Hardware, Ram};

use crate::agent::Approval;
use crate::script::{Builder, Script};
use crate::time::Clock;
use crate::transcript::Millis;

/// Thursday 5 November 2026, 21:30 in Chicago.
pub const CLOCK: Clock = Clock { now: 1_793_935_800_000, offset: -6 * 3600 };

const MIN: Millis = 60_000;
const HOUR: Millis = 60 * MIN;
const DAY: Millis = 24 * HOUR;

fn gpu(vendor: GpuVendor, name: &str, memory: GpuMemory, drives_display: bool, integrated: bool) -> Gpu {
    Gpu {
        vendor,
        name: name.into(),
        memory,
        drives_display,
        integrated,
        bandwidth: tasuke_models::hardware::known_bandwidth(name),
        pci: Some("0000:01:00.0".into()),
        device_index: Some(0),
        compute_capability: None,
    }
}

fn cpu(model: &str, physical: u32, logical: u32) -> Cpu {
    let mut c = Cpu { model: model.into(), physical_cores: physical, logical_cores: logical, ..Cpu::default() };
    c.flags.avx2 = true;
    c
}

/// A gaming laptop whose RTX 4060 (8 GB) drives its screen (its MUX switch
/// set to the discrete GPU), with 32 GB of memory.
pub fn laptop_8gb() -> Hardware {
    Hardware {
        gpus: vec![gpu(
            GpuVendor::Nvidia,
            "NVIDIA GeForce RTX 4060 Laptop GPU",
            GpuMemory::Dedicated { total: Bytes::mib(8188), free: Bytes::mib(7420) },
            true,
            false,
        )],
        ram: Ram { total: Bytes::mib(31_806), available: Bytes::mib(24_100) },
        cpu: cpu("Intel Core i7-13700H", 14, 20),
        notes: Vec::new(),
    }
}

/// A desktop whose RTX 5090 (32 GB) drives the display, with 64 GB.
pub fn desktop_32gb() -> Hardware {
    Hardware {
        gpus: vec![gpu(
            GpuVendor::Nvidia,
            "NVIDIA GeForce RTX 5090",
            GpuMemory::Dedicated { total: Bytes::mib(32_607), free: Bytes::mib(30_284) },
            true,
            false,
        )],
        ram: Ram { total: Bytes::mib(63_420), available: Bytes::mib(51_200) },
        cpu: cpu("AMD Ryzen 9 9950X 16-Core Processor", 16, 32),
        notes: Vec::new(),
    }
}

/// A small desktop on an AMD Ryzen AI Max+ 395, whose Radeon 8060S shares
/// 128 GB of memory with the processor.
pub fn apu_128gb() -> Hardware {
    Hardware {
        gpus: vec![gpu(
            GpuVendor::Amd,
            "AMD Radeon 8060S",
            GpuMemory::Unified { limit: Some(Bytes::gib(110)) },
            true,
            true,
        )],
        ram: Ram { total: Bytes::mib(126_560), available: Bytes::mib(110_000) },
        cpu: cpu("AMD Ryzen AI Max+ 395 w/ Radeon 8060S", 16, 32),
        notes: Vec::new(),
    }
}

/// A conversation as a fixture has it.
pub struct Conversation {
    pub title: &'static str,
    /// When its message was sent.
    pub at: Millis,
    pub text: &'static str,
    pub script: Script,
    pub act: bool,
    pub desktop: bool,
}

/// The notes in Memory.
pub fn notes() -> Vec<(String, &'static str, Millis)> {
    let n = CLOCK.now;
    vec![
        ("m1".into(), "The printer in the study is a Brother HL-L2350DW, on the home Wi-Fi.", n - 41 * DAY),
        ("m2".into(), "Their external monitor is a 27-inch 4K screen on USB-C, connected as DP-2.", n - 19 * DAY),
        ("m3".into(), "Uses fish as their shell.", n - 12 * DAY),
        ("m4".into(), "Asked never to touch ~/Videos when freeing space.", n - 4 * DAY),
        ("m5".into(), "Prefers the Day theme while it's light out.", n - 3 * HOUR),
    ]
}

const DF: &str = "Filesystem      Size  Used Avail Use% Mounted on
/dev/nvme0n1p2  476G  412G   40G  92% /
/dev/nvme0n1p2  476G  412G   40G  92% /home";

const DU_HOME: &str = "1.2G\t/home/sam/.local
3.4G\t/home/sam/Documents
11G\t/home/sam/Downloads
14G\t/home/sam/.cache
92G\t/home/sam/Videos
126G\t/home/sam";

const GENERATIONS: &str = "  14   2026-07-14 09:12:40
  15   2026-07-21 18:03:11
  16   2026-07-30 08:47:52
 …
  42   2026-10-29 07:58:03
  43   2026-11-02 19:24:17
  44   2026-11-05 08:10:45   (current)";

/// Why is my disk filling up? It looks, explains with a table, and asks to
/// collect the store's garbage.
pub fn disk() -> Script {
    Builder::new()
        .start()
        .think(
            "On Sekka a full disk is usually the Nix store keeping old system versions, then caches. \
             Look before guessing: the maintenance notes, what's free, where it went.",
        )
        .look("d1", "load_skill", json!({"name": "sekka-maintenance"}), "Loaded sekka-maintenance.", None, 40)
        .look("d2", "run_command", json!({"command": "df -h / /home", "purpose": "See how full the disk is"}), DF, Some(0), 90)
        .look(
            "d3",
            "run_command",
            json!({"command": "du -xh --max-depth=1 ~ | sort -h | tail -n 6", "purpose": "Find the largest folders at home"}),
            DU_HOME,
            Some(0),
            2400,
        )
        .look(
            "d4",
            "run_command",
            json!({"command": "nix-env -p /nix/var/nix/profiles/system --list-generations", "purpose": "Count the old system versions"}),
            GENERATIONS,
            Some(0),
            300,
        )
        .look(
            "d5",
            "run_command",
            json!({"command": "nix-store --gc --print-dead | wc -l", "purpose": "See how much of the store nothing uses"}),
            "18412",
            Some(0),
            5200,
        )
        .look("d6", "run_command", json!({"command": "journalctl --disk-usage"}), "Archived and active journals take up 48.0M in the file system.", Some(0), 60)
        .say(
            "Your disk is **92% full**: 412 GB used, 40 GB free. Most of it is in three places:\n\n\
             | Where | Size | What it is |\n\
             |---|---:|---|\n\
             | `/nix/store` | 248 GB | Software, kept for **31 system versions** back to 14 July |\n\
             | `~/Videos` | 92 GB | Your videos; I won't touch these |\n\
             | `~/.cache` | 14 GB | Caches that programs rebuild |\n\n\
             The store is what grew. Every update keeps the version before it so you can go back, \
             and nothing has removed the old ones since July.\n\n\
             Two steps, smallest first:\n\n\
             1. **Delete software nothing uses any more** (about 41 GB). It can't affect a system version you can boot.\n\
             2. **Remove system versions older than 30 days.** That frees most of the rest, but you can't go back to them. \
             Yukimi does this under *Storage → Clean Up…*, or I can.\n\n\
             I'll start with the first.",
        )
        .ask(
            "d7",
            "run_command",
            json!({
                "command": "nix-store --gc",
                "purpose": "Delete the 18,412 store paths that no system version or program uses (about 41 GB)"
            }),
            Approval::Ask { reason: "nix-store --gc deletes files from the Nix store".into() },
        )
        .runs("d7", "finding garbage collector roots...\ndeleting garbage...\n…\n18412 store paths deleted, 41.27 GiB freed", Some(0), 9000)
        .say("Done: **41 GB freed**, and the disk is 84% full. Shall I remove the system versions older than 30 days as well?")
        .finish(68.0)
        .otherwise("d7")
        .say("All right, nothing was deleted. When you want the space back, Yukimi's *Storage → Clean Up…* does the same.")
        .finish(68.0)
        .then()
        .build()
}

/// Clean up old system versions: root, with a password after Run.
pub fn generations() -> Script {
    Builder::new()
        .start()
        .look(
            "g1",
            "run_command",
            json!({"command": "nix-env -p /nix/var/nix/profiles/system --list-generations", "purpose": "List the system versions"}),
            GENERATIONS,
            Some(0),
            300,
        )
        .look("g2", "run_command", json!({"command": "df -h /"}), "Filesystem      Size  Used Avail Use% Mounted on\n/dev/nvme0n1p2  476G  412G   40G  92% /", Some(0), 80)
        .say(
            "You have **31 system versions**, the oldest from 14 July. Removing the 23 older than 30 days keeps the \
             8 newest in the boot menu, so you can still go back a month. It frees about **96 GB**.\n\n\
             This needs administrator rights, so the system asks for your password after you choose Run.",
        )
        .ask(
            "g3",
            "run_as_root",
            json!({
                "command": "nix-env -p /nix/var/nix/profiles/system --delete-generations 30d && nix-store --gc && /nix/var/nix/profiles/system/bin/switch-to-configuration boot",
                "purpose": "Remove the 23 system versions older than 30 days, delete the software only they used, and update the boot menu"
            }),
            Approval::AskRoot { reason: "It runs as administrator, and removed versions can't be booted again".into() },
        )
        .runs("g3", "removing profile version 14\n…\nremoving profile version 36\n9218 store paths deleted, 95.81 GiB freed\nupdating GRUB 2 menu...", Some(0), 14000)
        .say("Done: **96 GB freed**. The boot menu now lists the 8 versions from the last 30 days.")
        .finish(68.0)
        .otherwise("g3")
        .say("Nothing was removed. The versions stay in the boot menu.")
        .finish(68.0)
        .then()
        .build()
}

/// My sound stopped working: muted; unmuted once allowed.
pub fn sound() -> Script {
    Builder::new()
        .start()
        .think("No sound: check the services are up, then what the default output is and whether it's muted.")
        .look("s1", "load_skill", json!({"name": "sekka-troubleshooting"}), "Loaded sekka-troubleshooting.", None, 40)
        .look(
            "s2",
            "run_command",
            json!({"command": "systemctl --user is-active pipewire wireplumber pipewire-pulse"}),
            "active\nactive\nactive",
            Some(0),
            60,
        )
        .look(
            "s3",
            "run_command",
            json!({"command": "wpctl status"}),
            "Audio\n ├─ Devices:\n │      48. Family 17h/19h HD Audio Controller [alsa]\n ├─ Sinks:\n │  *   57. Speakers                       [vol: 0.62 MUTED]\n │      62. LG HDR 4K (DP-2)               [vol: 1.00]",
            Some(0),
            90,
        )
        .look("s4", "run_command", json!({"command": "wpctl get-volume @DEFAULT_AUDIO_SINK@"}), "Volume: 0.62 [MUTED]", Some(0), 40)
        .say(
            "The sound system is running and your **Speakers** are the output, but they're **muted** (at 62%). \
             Nothing is broken: a mute key or a volume change probably did it.",
        )
        .ask(
            "s5",
            "run_command",
            json!({"command": "wpctl set-mute @DEFAULT_AUDIO_SINK@ 0", "purpose": "Unmute the speakers"}),
            Approval::Ask { reason: "wpctl set-mute changes a sound setting".into() },
        )
        .runs("s5", "", Some(0), 120)
        .look("s6", "run_command", json!({"command": "wpctl get-volume @DEFAULT_AUDIO_SINK@"}), "Volume: 0.62", Some(0), 40)
        .say(
            "Sound is back, at 62%. If it goes quiet again, the speaker in the menu bar shows a slash when it's muted; \
             click it to unmute.",
        )
        .finish(71.0)
        .otherwise("s5")
        .say("Left muted. The speaker in the menu bar unmutes it whenever you like.")
        .finish(71.0)
        .then()
        .build()
}

const SETTINGS: &str = "[appearance]
theme = \"night\"
focus = \"amber\"
effects = \"lean\"
size = \"medium\"

[clock]
seconds = false";

const THEME_DIFF: &str = "@@ -1,5 +1,5 @@
 [appearance]
-theme = \"night\"
+theme = \"day\"
 focus = \"amber\"
 effects = \"lean\"
 size = \"medium\"";

/// The picture the agent's screenshot shows in the theme fixture: Aizome's
/// own gallery capture of Day (`docs/screenshots/07-day.png`), standing in
/// for what `aizomectl screenshot` would capture.
pub fn day_screenshot() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/screenshots/07-day.png")
}

/// Switch to the Day theme: a file change with its diff, then a look at
/// the screen to see it took.
pub fn day_theme() -> Script {
    let shot = crate::agent::ToolOutput { image: Some(day_screenshot()), ..Default::default() };
    Builder::new()
        .start()
        .look("t1", "load_skill", json!({"name": "aizome-settings"}), "Loaded aizome-settings.", None, 40)
        .look("t2", "read_file", json!({"path": "/home/sam/.config/aizome/settings.toml"}), SETTINGS, None, 20)
        .ask(
            "t3",
            "edit_file",
            json!({
                "path": "/home/sam/.config/aizome/settings.toml",
                "old": "theme = \"night\"",
                "new": "theme = \"day\"",
                "diff": THEME_DIFF
            }),
            Approval::Ask { reason: "It changes a file in your settings".into() },
        )
        .runs("t3", "Changed 1 line.", None, 30)
        .event(220, crate::agent::Event::ToolCall {
            id: "t4".into(),
            call: crate::agent::ToolCall::new("screenshot", json!({})),
            needs: Approval::None,
        })
        .event(30, crate::agent::Event::ToolStarted { id: "t4".into() })
        .event(600, crate::agent::Event::ToolOutput { id: "t4".into(), output: shot })
        .say(
            "Done: the desktop is in **Day** now, and it stays that way until you change it.\n\n\
             To have it follow the clock instead, choose **Auto** in *Settings → Appearance → Theme*: \
             Day from 07:00 to 19:00, or from sunrise to sunset.",
        )
        .finish(68.0)
        .otherwise("t3")
        .say("Left as it was, in Night.")
        .finish(68.0)
        .then()
        .build()
}

/// How do I share my screen? Steps, from the desktop's notes.
pub fn share_screen() -> Script {
    Builder::new()
        .start()
        .look("h1", "load_skill", json!({"name": "aizome-desktop"}), "Loaded aizome-desktop.", None, 40)
        .say(
            "In a video call in your browser, choose the call's **Share screen** button. Then Aizome asks what to share:\n\n\
             1. **Click a screen** to share all of it, or\n\
             2. press **Space**, then **click a window** to share just that one.\n\
             3. **Esc** shares nothing.\n\n\
             While you share, a blue chip shows in the Deskbar's header. Press it and choose **Stop Sharing** to end it.\n\n\
             > [!TIP]\n\
             > Sharing one window keeps notifications and other windows out of the call.",
        )
        .finish(70.0)
        .build()
}

/// A question with no tools: what's a swap file?
pub fn swap() -> Script {
    Builder::new()
        .start()
        .think("General question. Explain plainly, then say what Sekka does.")
        .say(
            "A **swap file** is space on the disk that the system uses as overflow memory. When programs need more \
             memory than you have, the least-used parts move to swap, so nothing has to close.\n\n\
             Disk is far slower than memory, so a computer that swaps a lot feels sluggish. It's a safety net, not extra memory.\n\n\
             Sekka also compresses memory before it reaches the disk (*zram*), which is much faster than swapping.",
        )
        .finish(70.0)
        .build()
}

/// Give my computer a check-up: several looks, then a short report.
pub fn checkup() -> Script {
    Builder::new()
        .start()
        .think("A check-up: the disk, memory, failed services, recent errors, updates. Read-only.")
        .look("c1", "run_command", json!({"command": "df -h / /home"}), DF, Some(0), 90)
        .look("c2", "run_command", json!({"command": "free -h"}), "               total        used        free      shared  buff/cache   available\nMem:            62Gi        11Gi        38Gi       1.2Gi        14Gi        50Gi\nSwap:           31Gi          0B        31Gi", Some(0), 30)
        .look("c3", "run_command", json!({"command": "systemctl --failed --no-legend"}), "", Some(0), 70)
        .look("c4", "run_command", json!({"command": "systemctl --user --failed --no-legend"}), "", Some(0), 60)
        .look("c5", "run_command", json!({"command": "journalctl -b -p err --no-pager | tail -n 20"}), "Nov 05 08:11:02 studio kernel: ACPI BIOS Error (bug): Could not resolve symbol [\\_SB.PCI0.GPP0.SWUS], AE_NOT_FOUND", Some(0), 400)
        .look("c6", "run_command", json!({"command": "nixos-version"}), "26.05.20261104.6f1a2c3 (Yarara)", Some(0), 20)
        .say(
            "Your computer is in good health. In short:\n\n\
             - **Disk**: 92% full, 40 GB free. The most worth doing: old system versions take about 140 GB. \
             Ask me *why is my disk filling up?* and I'll walk you through it.\n\
             - **Memory**: 11 GB of 62 GB in use; nothing is being swapped.\n\
             - **Services**: none failed.\n\
             - **Errors since startup**: one, an ACPI message from the firmware that's harmless and common on this kind of board.\n\
             - **System**: Sekka on NixOS 26.05, updated yesterday.",
        )
        .finish(68.0)
        .build()
}

/// What a fixture says to a message it has no script for.
pub fn unscripted() -> Script {
    Builder::new()
        .start()
        .say(
            "This window is running a **fixture**: Tasuke's agent isn't connected, so it can only play its scripted \
             conversations. Try one of the suggestions on a new conversation.",
        )
        .finish(68.0)
        .build()
}

/// The script for a message, in a fixture: a suggestion's, or none.
pub fn script_for(text: &str) -> Script {
    let t = text.to_lowercase();
    if t.contains("check-up") || t.contains("checkup") {
        checkup()
    } else if t.contains("disk") {
        disk()
    } else if t.contains("day theme") {
        day_theme()
    } else if t.contains("share my screen") {
        share_screen()
    } else if t.contains("sound") {
        sound()
    } else if t.contains("swap") {
        swap()
    } else if t.contains("system versions") {
        generations()
    } else {
        unscripted()
    }
}

/// The fixture's earlier conversations, oldest first. The last one is
/// the conversation fixtures `diagnose` shows (still waiting on its card).
pub fn conversations() -> Vec<Conversation> {
    let n = CLOCK.now;
    let c = |title, at, text, script, desktop| Conversation { title, at, text, script, act: true, desktop };
    vec![
        c("What's a swap file?", n - 8 * DAY, "What's a swap file?", swap(), false),
        c("How do I share my screen?", n - 3 * DAY - 2 * HOUR, "How do I share my screen?", share_screen(), false),
        c("My sound stopped working", n - DAY - 3 * HOUR, "My sound stopped working", sound(), false),
        c("Switch to the Day theme", n - 3 * HOUR, "Switch to the Day theme", day_theme(), true),
        c("Clean up old system versions", n - MIN, "Clean up old system versions", generations(), false),
        c("Why is my disk filling up?", n - 4 * MIN, "Why is my disk filling up?", disk(), false),
    ]
}
