//! Aizome's apps: names, meanings and what each does. Every claim comes from
//! desktop/aizome/docs/apps.md, the app's own docs (koe.md, koyomi.md,
//! shiori.md, apps/hotaru/README.md) or desktop/aizome/docs/performance.md;
//! keep it so. Annai ships: the NixOS module installs its autostart entry.

use crate::{Frame, Site, win};
use anyhow::Result;

pub struct App {
    pub slug: &'static str,
    pub name: &'static str,
    pub kanji: &'static str,
    pub generic: &'static str,
    pub meaning: &'static str,
    /// The icon the desktop installs, in this repository.
    pub icon: &'static str,
    pub summary: &'static str,
    pub headline: &'static str,
}

impl App {
    pub fn id(&self) -> String {
        format!("org.aizome.{}", self.name)
    }
    pub fn path(&self) -> String {
        format!("/apps/{}/", self.slug)
    }
}

/// The Toolchest's icon: Aizome's mark on its tile.
pub const TOOLCHEST_ICON: &str = "desktop/aizome/icons/apps/org.aizome.Toolchest.svg";

pub const APPS: [App; 19] = [
    App {
        slug: "settei",
        name: "Settei",
        kanji: "設定",
        generic: "Settings",
        meaning: "settings",
        icon: "desktop/aizome/icons/apps/org.aizome.Settei.svg",
        summary: "Every setting in one window, laid out as macOS System Settings and searchable from the palette.",
        headline: "Settings that apply as you change them.",
    },
    App {
        slug: "yukimi",
        name: "Yukimi",
        kanji: "雪見",
        generic: "Software",
        meaning: "snow-viewing, through a shōji's low pane of glass",
        icon: "desktop/aizome/icons/apps/org.aizome.Yukimi.svg",
        summary: "Sekka's software manager: see what's installed and why, find software, update, go back and free up space.",
        headline: "A window onto your system.",
    },
    App {
        slug: "hako",
        name: "Hako",
        kanji: "箱",
        generic: "Files",
        meaning: "box",
        icon: "desktop/aizome/icons/apps/org.aizome.Hako.svg",
        summary: "A file manager in NeXT's spirit, with Miller columns, Quick Look on Space and the Shelf.",
        headline: "Files in columns, as NeXT kept them.",
    },
    App {
        slug: "sumi",
        name: "Sumi",
        kanji: "墨",
        generic: "Text Editor",
        meaning: "ink",
        icon: "desktop/aizome/icons/apps/org.aizome.Sumi.svg",
        summary: "A text editor that styles Markdown as you type and brings every document back after a crash.",
        headline: "Write, and Sumi keeps every word.",
    },
    App {
        slug: "hotaru",
        name: "Hotaru",
        kanji: "蛍",
        generic: "Terminal",
        meaning: "firefly",
        icon: "desktop/aizome/apps/hotaru/data/org.aizome.Hotaru.svg",
        summary: "Aizome's own terminal: tabs and split panes, search through the whole scrollback, and Super shortcuts.",
        headline: "The terminal, native to Aizome.",
    },
    App {
        slug: "fusen",
        name: "Fusen",
        kanji: "付箋",
        generic: "Notes",
        meaning: "sticky notes",
        icon: "desktop/aizome/icons/apps/org.aizome.Fusen.svg",
        summary: "Notes as plain Markdown files in ~/Notes, any of which can float as a sticky.",
        headline: "Notes that stay plain files.",
    },
    App {
        slug: "koyomi",
        name: "Koyomi",
        kanji: "暦",
        generic: "Calendar",
        meaning: "almanac, calendar",
        icon: "desktop/aizome/icons/apps/org.aizome.Koyomi.svg",
        summary: "Month, week, day and year views over iCalendar files any program can read.",
        headline: "A calendar that keeps your events in files.",
    },
    App {
        slug: "tokei",
        name: "Tokei",
        kanji: "時計",
        generic: "Clock",
        meaning: "clock",
        icon: "desktop/aizome/icons/apps/org.aizome.Tokei.svg",
        summary: "World clocks, alarms that fire with the app closed, a timer and a stopwatch.",
        headline: "Time, here and everywhere else.",
    },
    App {
        slug: "soroban",
        name: "Soroban",
        kanji: "算盤",
        generic: "Calculator",
        meaning: "abacus",
        icon: "desktop/aizome/icons/apps/org.aizome.Soroban.svg",
        summary: "Basic, Scientific and Programmer modes, with a paper tape of everything you worked out.",
        headline: "A calculator that keeps the paper tape.",
    },
    App {
        slug: "tenki",
        name: "Tenki",
        kanji: "天気",
        generic: "Weather",
        meaning: "weather",
        icon: "desktop/aizome/icons/apps/org.aizome.Tenki.svg",
        summary: "The weather now, for 48 hours and for ten days, from Open-Meteo, with no account or key.",
        headline: "The weather, now and ten days out.",
    },
    App {
        slug: "tansu",
        name: "Tansu",
        kanji: "箪笥",
        generic: "Disks",
        meaning: "chest of drawers (partitions as drawers)",
        icon: "desktop/aizome/icons/apps/org.aizome.Tansu.svg",
        summary: "Partition, format and resize disks, with every change listed in plain words before it happens.",
        headline: "Disks, drawer by drawer.",
    },
    App {
        slug: "kura",
        name: "Kura",
        kanji: "蔵",
        generic: "Disk Usage",
        meaning: "storehouse",
        icon: "desktop/aizome/icons/apps/org.aizome.Kura.svg",
        summary: "See what fills a disk as rings, gather what can go, and let agents read the same index.",
        headline: "What fills your disk, and what can go.",
    },
    App {
        slug: "myaku",
        name: "Myaku",
        kanji: "脈",
        generic: "Activity",
        meaning: "pulse",
        icon: "desktop/aizome/icons/apps/org.aizome.Myaku.svg",
        summary: "Every app with its processes: processor, memory, energy, disk and graphics, card by card.",
        headline: "The pulse of every app.",
    },
    App {
        slug: "nisshi",
        name: "Nisshi",
        kanji: "日誌",
        generic: "Logs",
        meaning: "log book, a day's record",
        icon: "desktop/aizome/icons/apps/org.aizome.Nisshi.svg",
        summary: "The system's journal and /var/log, filtered as you type, with filters suggested by what wrote each message.",
        headline: "The journal, made readable.",
    },
    App {
        slug: "shashin",
        name: "Shashin",
        kanji: "写真",
        generic: "Images",
        meaning: "photograph",
        icon: "desktop/aizome/icons/apps/org.aizome.Shashin.svg",
        summary: "The image viewer: crisp at 100%, quick from picture to picture, and turns without re-encoding.",
        headline: "Pictures, pixel for pixel.",
    },
    App {
        slug: "shiori",
        name: "Shiori",
        kanji: "栞",
        generic: "Documents",
        meaning: "bookmark, the slip that keeps your place in a book",
        icon: "desktop/aizome/icons/apps/org.aizome.Shiori.svg",
        summary: "The PDF reader: drawn in Rust, a process per document, and it opens where you left off.",
        headline: "PDFs that open where you left off.",
    },
    App {
        slug: "utsushi",
        name: "Utsushi",
        kanji: "映し",
        generic: "Media Player",
        meaning: "projecting, as onto a screen",
        icon: "desktop/aizome/icons/apps/org.aizome.Utsushi.svg",
        summary: "Films and music, played with as little work as the machine can do them with.",
        headline: "Films and music, with the lightest touch.",
    },
    App {
        slug: "koe",
        name: "Koe",
        kanji: "声",
        generic: "Dictation",
        meaning: "voice",
        icon: "desktop/aizome/icons/apps/org.aizome.Koe.svg",
        summary: "Speak, and the words are typed into the app in front, recognised on this computer.",
        headline: "Speak, and it's typed.",
    },
    App {
        slug: "annai",
        name: "Annai",
        kanji: "案内",
        generic: "Welcome",
        meaning: "guidance",
        icon: "desktop/aizome/icons/apps/org.aizome.Annai.svg",
        summary: "A short tour of Aizome's ideas at your first login, each with a live illustration.",
        headline: "A welcome that shows the ideas.",
    },
];

/// Provenance shared by several captions.
pub const VM: &str = "From Sekka's screenshot VM (nix build .#screenshots-aizome): a disposable NixOS VM built from the desktop module installed systems import, at 2880 × 1800 and scale 2, composited in software and driven through Aizome's own input path.";
pub const WALKTHROUGH: &str = "From Aizome's end-to-end walkthrough of 18 October 2026: the real session (compositor, shell and apps, release builds) running headless at 2880 × 1800, scale 2, driven through aizomectl input as a person would, and captured with grim. Its terminal is foot, the default before Hotaru.";
pub const NISSHI: &str = "Nisshi's release build of 29 October 2026, alone under a private headless sway at 1440 × 900, scale 1, drawing its own menu bar. It read a made-up boot through a stand-in journalctl, so no message of a real computer is shown.";
pub const GROUP: &str = "From the compositor's tab-group check (desktop/aizome/docs/ipc.md): Google Chrome and Soroban in a private headless compositor at 1440 × 900, grouped, switched by clicking their tabs, and torn apart again.";

fn facts(items: &[&str]) -> String {
    let mut s = String::from(r#"<ul class="facts">"#);
    for i in items {
        s.push_str("<li>");
        s.push_str(i);
        s.push_str("</li>");
    }
    s.push_str("</ul>");
    s
}

/// What the app's page says beneath its header.
pub fn body(site: &Site, app: &App) -> Result<String> {
    let mut b = String::new();
    match app.slug {
        "settei" => {
            b += &site.shot(
                "settei.png",
                "Settei on its Appearance pane: Night, Day, Auto and High Contrast drawn as small desktops with Night chosen; the focus colour Kohaku amber among four; the effects tiers Full, Lean and Austere with Lean chosen; and Size.",
                &format!("Settei at Appearance, cropped to its window with its tab, frame and shadow. {VM}"),
                Frame::Lead,
            )?;
            b += r#"<div class="tiles">"#;
            b += &win(
                "panes",
                "Panes",
                "span-2",
                &format!(
                    "<p>A sidebar with a search field, your account's card and the panes in groups, each with its glyph on a small dyed tile; the pane you pick on the right, as a form. Changes reach the system as soon as you make them.</p>{}",
                    facts(&[
                        "<strong>Network</strong>: Wi-Fi, wired, VPN and WireGuard; joining with a password or by a hidden network's name; each connection's IPv4, IPv6, DNS, metering and private hardware address; airplane mode and a hotspot.",
                        "<strong>Bluetooth</strong>: nearby and paired devices, pairing with passkey confirmation, connecting, and their batteries.",
                        "<strong>Sound</strong>: outputs and inputs with volume, balance and mute, a level meter for the microphone, per-app volumes and the alert sound.",
                        "<strong>Displays</strong>: arrange them by dragging; resolution and refresh; integer scales, with fractional ones when you want them; rotation, mirroring and Night Light. A change that could leave a screen blank undoes itself after 15 seconds unless you keep it.",
                        "<strong>Appearance</strong>: Night, Day, Auto by sunset and sunrise, and High Contrast; the focus colour (amber, cyan, rose, lime); the effects tier; Size; and a wallpaper for Night and another for Day.",
                        "<strong>Desktop &amp; Deskbar</strong>, <strong>Menu Bar</strong> and <strong>Search</strong>: Desks and their folders, where the Deskbar stands and how wide, the clock and the systray, and what the palette searches.",
                        "<strong>Performance</strong>: the renderer, GPU or software, and live frame statistics against the 4 ms budget.",
                        "<strong>Keyboard</strong>, <strong>Mouse</strong> and <strong>Trackpad</strong>: layouts, repeat, modifier keys, Caps Lock and Compose, and every one of Aizome's shortcuts, editable.",
                        "<strong>Battery &amp; Power</strong>, <strong>Lock Screen</strong>, <strong>Users &amp; Groups</strong>, <strong>Printers &amp; Scanners</strong> and <strong>Accessibility</strong>.",
                        "<strong>General</strong>: About, Software Update (in Yukimi), Storage (in Kura), Sekka's boot splash, Date &amp; Time, Language &amp; Region, Default Apps and Login Items.",
                    ])
                ),
            );
            b += &win(
                "find",
                "Find any setting",
                "",
                "<p>Search filters the sidebar and highlights what it found. Every pane and its main options are in the command palette too: after ⟦❖ Space⟧, type what you want (“Keyboard › Remap Caps Lock”) and Settei opens at that place.</p><p>Scripts open a pane directly: <code>aizome-settei --pane keyboard.modifiers</code>.</p>",
            );
            b += &win(
                "keyboards",
                "Each keyboard its own",
                "",
                "<p>Modifier keys, Caps Lock and the Compose key can differ from keyboard to keyboard. <strong>Set Up…</strong> asks you to press the key just left of the space bar: Alt there, as on a PC, suggests swapping Alt and Super; ⌘ Command or the Windows key needs nothing; Control suggests swapping Control and Super. Plug in or pair a keyboard and Aizome offers the same in a notification. Layouts, repeat and shortcuts are every keyboard's.</p>",
            );
            b += &win(
                "beyond",
                "Beyond Aizome's own apps",
                "",
                "<p>GTK and libadwaita apps follow the theme and the focus colour, which the shell writes for them. On NixOS, a local administrator changes the time zone, the computer's name, the language, the keyboard and networks without a password prompt, as GNOME allows.</p>",
            );
            b += "</div>";
            b += &site.shot(
                "settei-displays.png",
                "Settei's Displays pane on a freshly installed Sekka: one QEMU monitor in the arrangement, its resolution, and Use as main display; Firefox, Hako and Annai's welcome listed in the Deskbar beside it.",
                "Settei at Displays in the installed Aizome session of an encrypted Sekka installation on 21 October 2026: a QEMU VM driven only with keyboard and pointer events, with no automatic login. The whole display, unaltered.",
                Frame::Window,
            )?;
        }
        "yukimi" => {
            b += &site.shot(
                "yukimi.png",
                "Yukimi's Overview of a test machine named sekka-demo: a banner of falling snow, then its packages, freshness, store size and generations, and a bar of what fills the store.",
                &format!("Yukimi in Aizome, reading the VM sekka-demo, cropped to its window. {VM}"),
                Frame::Lead,
            )?;
            b += r#"<div class="tiles">"#;
            b += &win(
                "pages",
                "Pages",
                "span-2",
                &facts(&[
                    "<strong>Overview</strong>: the system at a glance, its NixOS version and kernel, how many packages it has, how fresh its Nixpkgs is and how much room the store takes.",
                    "<strong>Installed</strong>: what's installed and why: chosen for everyone, just for you, or brought by your desktops and settings.",
                    "<strong>Discover</strong>: find software and install it.",
                    "<strong>Updates</strong>: Update Sekka in one action. <strong>Early updates</strong> follows Sekka's <code>main</code> branch as soon as changes are merged.",
                    "<strong>History</strong>: every generation, and going back to one.",
                    "<strong>Storage</strong>: what fills the store, and removing old generations safely.",
                    "<strong>Desktops</strong>: which desktops are installed, which starts by default, the login screen and the boot splash. Picking changes nothing; Apply asks, then builds once, and a change that needs a restart offers Restart Now.",
                ]),
            );
            b += &win(
                "safe",
                "Changes you can trust",
                "",
                "<p>System changes go through Sekka's privileged helper, after an administrator's password in Aizome's own dialog. The helper refuses any setting the system doesn't declare. Cancelling and refusing are told apart, and a failed change keeps the helper's report.</p>",
            );
            b += &win(
                "cli",
                "From a terminal",
                "",
                "<p><code>aizome-yukimi --choose calamares.loginScreen=aizome --early</code> opens Yukimi with that change proposed. Nothing changes until it's confirmed and an administrator's password is entered.</p><p>Every Sekka desktop uses this Yukimi; in Aizome its commands use Super. <a href=\"https://sekka.computer/yukimi/\">Yukimi on sekka.computer</a></p>",
            );
            b += "</div>";
        }
        "hako" => {
            b += r#"<div class="tiles">"#;
            b += &win(
                "does",
                "Everything a file manager does",
                "span-2",
                &facts(&[
                    "Miller columns as the main view, with list and icon views beside them, a path bar, and Quick Look on ⟦Space⟧: images, text and a PDF's first page.",
                    "A sidebar of Places, the current Desk's folder, disks, Recent and Trash.",
                    "Search with the same engine as the command palette.",
                    "Copy, move, trash and restore, delete, rename, new folder, duplicate, compress and extract, with a progress panel.",
                    "Open With and Get Info: size, kind, dates and permissions.",
                    "Drag and drop with other apps, the desktop and the Shelf.",
                    "Image thumbnails kept in the shared freedesktop cache, so every app that reads it sees them.",
                    "Tabs are Aizome's own tab groups, so there are none inside.",
                ]),
            );
            b += &win(
                "columns",
                "Columns that stay put",
                "",
                "<p>Refreshing a listing keeps a still-selected folder's deeper columns open. If that folder goes, its columns close and the keyboard returns to the parent that's left.</p>",
            );
            b += &win(
                "encrypted",
                "Encrypted drives",
                "",
                "<p>An encrypted removable volume asks for its passphrase before it mounts, lets you try again after a typo, and stays in the sidebar once unlocked.</p>",
            );
            b += &win(
                "chooser",
                "Hako in every app",
                "span-2",
                "<p>Browsers, Electron apps, Zed and Flatpaks open and save files in Aizome's own chooser, built from Hako's model and look: its sidebar, its path bar and a list of the folder, over the app's window. <a href=\"/desktop/#chooser\">The file chooser</a></p>",
            );
            b += "</div>";
        }
        "sumi" => {
            b += &site.shot(
                "desktop.png",
                "The Aizome desktop with Sumi focused on a Markdown file, architecture.md: its headings, code and line numbers styled; Hotaru and Files stacked in one group below it; Tenki's notification filed in the Deskbar.",
                &format!("Sumi focused on the whole desktop. {VM}"),
                Frame::Lead,
            )?;
            b += r#"<div class="tiles">"#;
            b += &win(
                "editor",
                "The editor",
                "span-2",
                &facts(&[
                    "A window per document, with a toolbar of the language, bold, italic and code, and “Ln 48, Col 12 · UTF-8 · LF” at its right.",
                    "Line numbers, and the current line on a band of the focus colour.",
                    "Markdown styled as you type: headings, code and emphasis.",
                    "Syntax colouring for Rust, Python, JavaScript and TypeScript, C, Nix, shell, TOML, JSON and YAML.",
                    "The Format menu: Bold ⟦❖ B⟧, Italic ⟦❖ I⟧, Code ⟦❖ E⟧, Link ⟦❖ K⟧, headings and quotes, and Copy Style and Paste Style (⟦⌥ ❖ C⟧, ⟦⌥ ❖ V⟧).",
                    "Find and Replace with regular expressions (⟦⌥ ❖ F⟧).",
                ]),
            );
            b += &win(
                "kept",
                "Nothing lost",
                "",
                "<p>Unsaved text is kept as you type, written 0.8 seconds after the last change. After a crash, a logout, or Quit, which keeps everything rather than asking, your documents come back as they were, Untitled ones included. Closing a window with unsaved changes asks.</p>",
            );
            b += &win(
                "files",
                "Files as they are",
                "",
                "<p>Files are read as UTF-8, UTF-16 or Latin-1 and keep their line endings, LF, CRLF or CR, until you switch them in the File menu. Saves are atomic, follow symbolic links and keep the file's permissions. What Sumi opens and saves goes to Open Recent and to the desktop's shared list of recent files.</p>",
            );
            b += "</div>";
        }
        "hotaru" => {
            b += r#"<div class="tiles">"#;
            b += &win(
                "native",
                "Built with Aizome",
                "span-2",
                "<p>Native application menus, tab controls, search fields, the theme's colours, window activation and accessibility labels, with no GTK and no web view. ⟦❖ ↩⟧ opens a tab in the last-used Hotaru window on the current Desk, or a new window there when none exists, starting in the Desk's folder. File › New Window (⟦❖ N⟧) opens a separate window for another work context. Programs that need a terminal open in Hotaru too. foot stays installed, and Settei › General › Default Apps can choose it instead; ⟦❖ ↩⟧ then opens foot. Either opens in fish, set up as Omarchy sets it up: a Starship prompt, vi keys, and fzf searches of your history and files. Yukimi's Desktops page can switch it back to bash.</p>",
            );
            b += &win(
                "panes",
                "Tabs and panes",
                "",
                "<p>A tab splits into panes, right with ⟦❖ D⟧ or down with ⟦⇧ ❖ D⟧, with native dividers to drag. Zoom Pane (⟦⇧ ❖ ↩⟧) lets one fill its tab. When a program ends its pane closes, then its tab, then the window; one that fails as it starts keeps its pane so you can read why.</p><p>Bash, Zsh and Fish report their working directory without changes to your dotfiles, so new tabs and splits open where you were.</p>",
            );
            b += &win(
                "find",
                "Find and copy",
                "",
                "<p>Find (⟦❖ F⟧) searches the pane's whole scrollback on a worker, across wrapped lines, and copies a match even when it's far above the screen. Copy gives plain text and HTML together; Edit › Copy Format offers text with the terminal's formatting too.</p><p>Selecting text makes it the primary selection, which a middle click pastes. A file dropped on a pane pastes as its quoted path.</p>",
            );
            b += &win(
                "talk",
                "Programs that talk to the desktop",
                "",
                "<p>Editors and tmux use the clipboard through OSC 52, even over SSH: writing is allowed, and reading asks you first, showing what would be read. Settings chooses Ask, Allow or Never for each.</p><p>Hyperlinks (OSC 8) show their target in the status bar and open with ⟦❖⟧-click, as do plain URLs and paths. A long command can notify the desktop when it finishes (OSC 9 or OSC 777) while its pane isn't the one in use.</p>",
            );
            b += &win(
                "settings",
                "Settings",
                "",
                "<p>Hotaru › Settings… (⟦❖ ,⟧) has four pages laid out as Settei's: General; Text (family, size, weight, ligatures, fallback); Appearance (Follow Desktop, Night, Day or High Contrast, each shown as a small terminal; the cursor's shape and blink); and Shortcuts, where any command takes a new key. Changes reach every pane in that window at once and are saved in <code>~/.config/aizome/hotaru.toml</code>; ⟦⇧ ❖ ,⟧ reads saved changes in other open windows too. CJK and colour emoji fallback fonts come with Hotaru.</p>",
            );
            b += "</div>";
            b += &win(
                "keys",
                "Its keys",
                "",
                r#"<p>Aizome's Super shortcuts are the defaults, and the familiar Linux ones (Ghostty's, GNOME Terminal's) work too. Plain Control-C stays the terminal's.</p>
<table class="keytable"><tbody>
<tr><th scope="row">Settings, reload the configuration</th><td>⟦❖ ,⟧ ⟦⇧ ❖ ,⟧</td></tr>
<tr><th scope="row">New tab</th><td>⟦❖ T⟧</td></tr>
<tr><th scope="row">Split right, split down</th><td>⟦❖ D⟧ ⟦⇧ ❖ D⟧</td></tr>
<tr><th scope="row">Close the pane, close the tab, zoom the pane</th><td>⟦❖ W⟧ ⟦⌥ ❖ W⟧ ⟦⇧ ❖ ↩⟧</td></tr>
<tr><th scope="row">Previous tab, next tab</th><td>⟦⇧ ❖ [⟧ ⟦⇧ ❖ ]⟧</td></tr>
<tr><th scope="row">Tab 1 to 8, the last tab</th><td>⟦❖ 1⟧ … ⟦❖ 8⟧ ⟦❖ 9⟧</td></tr>
<tr><th scope="row">Previous pane, next pane</th><td>⟦❖ [⟧ ⟦❖ ]⟧</td></tr>
<tr><th scope="row">The pane to the left, right, above, below</th><td>⟦⇧ ❖ ←⟧ ⟦⇧ ❖ →⟧ ⟦⇧ ❖ ↑⟧ ⟦⇧ ❖ ↓⟧</td></tr>
<tr><th scope="row">Copy, paste, copy everything</th><td>⟦❖ C⟧ ⟦❖ V⟧ ⟦⇧ ❖ C⟧</td></tr>
<tr><th scope="row">Find, next, previous, find the selection</th><td>⟦❖ F⟧ ⟦❖ G⟧ ⟦⇧ ❖ G⟧ ⟦❖ E⟧</td></tr>
<tr><th scope="row">Clear the screen, reset the terminal</th><td>⟦❖ K⟧ ⟦⌥ ❖ R⟧</td></tr>
<tr><th scope="row">Larger text, smaller, actual size</th><td>⟦❖ +⟧ ⟦❖ −⟧ ⟦❖ 0⟧</td></tr>
<tr><th scope="row">Scroll to the top, the bottom</th><td>⟦❖ Home⟧ ⟦❖ End⟧</td></tr>
<tr><th scope="row">Scroll a page</th><td>⟦⇧ PgUp⟧ ⟦⇧ PgDn⟧</td></tr>
</tbody></table>"#,
            );
        }
        "fusen" => {
            b += r#"<div class="tiles">"#;
            b += &win(
                "notes",
                "Notes",
                "span-2",
                "<p>Notes are Markdown files in <code>~/Notes</code>, so they stay plain, searchable, and readable by other programs and agents. Beside the editor, a list with search and pinned notes, folders, checklists and a preview line.</p>",
            );
            b += &win(
                "stickies",
                "Stickies",
                "",
                "<p>Any note can float as a sticky: a small window on its colour's paper (amber, kamenozoki, hanada, green or rose) that stays above other windows and comes back where it was, at your next login too.</p>",
            );
            b += &win(
                "plain",
                "Plain files",
                "",
                "<p>A note's file is named after its title and renamed with it; folders are subdirectories. Pins and colours live in one small file beside them, so the notes stay plain. Files another program changes or adds show within three seconds, and a deleted note goes to the Trash. A click on a task's <code>[ ]</code> ticks it; ⟦❖⟧-click opens a link.</p>",
            );
            b += &win(
                "anywhere",
                "From anywhere",
                "span-2",
                "<p>Select text in any app and choose New Note with Selection from the app menu's Services. <code>aizome-fusen --new TEXT</code> and <code>--new-from-stdin</code> make notes from scripts. If a note can't be saved, Fusen keeps the draft, says why and offers Retry.</p>",
            );
            b += "</div>";
        }
        "koyomi" => {
            b += r#"<div class="tiles">"#;
            b += &win(
                "views",
                "Views and alerts",
                "span-2",
                "<p>Month, week, day and year views. Alerts fire as desktop notifications through a small user service, whether or not the window is open: relative to the start or the end, or at a set time, and an all-day event's on the wall clock. Week numbers and the first day of the week follow your locale.</p>",
            );
            b += &win(
                "files",
                "Your files",
                "",
                "<p>One calendar per file, in <code>~/.local/share/aizome/koyomi</code>, as iCalendar any program can read. Drop an <code>.ics</code> file into the folder and it's a calendar. Koyomi writes a whole file at a time and reads it again when another program changes it. Colours are the theme's dyes, with an Apple colour beside each for other programs.</p>",
            );
            b += &win(
                "reads",
                "Reads what others write",
                "",
                "<p>Exports from Google Calendar, Apple Calendar and Outlook open as they should: time zones however they're written (IANA names, Windows names, a file's own rules), recurrence rules down to the last Friday of the month, exceptions and changed instances. Recurring events keep their wall-clock time across daylight saving changes. What Koyomi doesn't use, attendees and to-dos included, is kept and written back unchanged.</p>",
            );
            b += "</div>";
        }
        "tokei" => {
            b += &site.shot(
                "tokei-tenki.png",
                "Tokei's World Clock beside Tenki: the time here and in New York, London and Tokyo, each with day or night; Tenki showing Chicago, clear at 43°, the next hours and ten days.",
                &format!("Tokei beside Tenki. {WALKTHROUGH} Tenki's forecast came from Open-Meteo."),
                Frame::Lead,
            )?;
            b += r#"<div class="tiles">"#;
            b += &win(
                "clocks",
                "World clocks",
                "",
                "<p>Search for a city and it joins the list with day or night and its offset from here. ⟦⇧ ❖ ↑⟧ and ⟦⇧ ❖ ↓⟧ move the selected city. The menu bar's clock panel shows them too.</p>",
            );
            b += &win(
                "alarms",
                "Alarms",
                "",
                "<p>Repeat days, a label and a sound. Alarms are scheduled as systemd user timers, so they fire with Tokei closed, and wake the computer from sleep when you ask them to.</p>",
            );
            b += &win(
                "timer",
                "Timer and stopwatch",
                "span-2",
                "<p>A timer, and a stopwatch with laps, in IBM Plex Mono's tabular digits, so nothing shifts as the numbers change. The world clock's tick takes 0.04 ms of the frame budget.</p>",
            );
            b += "</div>";
        }
        "soroban" => {
            b += &site.shot(
                "group-soroban.png",
                "Soroban in Basic mode with its empty paper tape, its tab amber beside Google Chrome's idle tab in the same group.",
                &format!("Soroban stacked with Google Chrome. {GROUP}"),
                Frame::Lead,
            )?;
            b += r#"<div class="tiles">"#;
            b += &win(
                "modes",
                "Three modes",
                "",
                "<p>Basic, Scientific and Programmer: hexadecimal, decimal, octal and binary, bitwise operations and the word size. Type whole expressions; Soroban is keyboard first.</p>",
            );
            b += &win(
                "tape",
                "The paper tape",
                "",
                "<p>Every calculation goes on a paper tape beside the keys, to copy or save. The command palette does arithmetic with the same engine: type a sum after ⟦❖ Space⟧ and its first row is the answer.</p>",
            );
            b += "</div>";
        }
        "tenki" => {
            b += &site.shot(
                "tokei-tenki.png",
                "Tenki showing Chicago: clear at 43°, high 62 and low 37, the next hours, ten days with their ranges, and an offer to use your location; Tokei's world clocks beside it.",
                &format!("Tenki beside Tokei. {WALKTHROUGH} The forecast came from Open-Meteo."),
                Frame::Lead,
            )?;
            b += r#"<div class="tiles">"#;
            b += &win(
                "place",
                "The place",
                "span-2",
                "<p>The temperature large in Archivo light, the condition, the high and the low; an hourly strip with rain bars for 48 hours; ten days with their ranges on one scale in the dye ramp, kamenozoki to kohaku; and tiles for feels-like, UV, wind with a compass, humidity, the sun and precipitation. Forecasts come from Open-Meteo, which needs no account or key.</p>",
            );
            b += &win(
                "places",
                "Places",
                "",
                "<p>Search for a city to keep it, and drag your places into order. Your current location is pinned first once you allow it.</p>",
            );
            b += &win(
                "private",
                "Your location, asked once",
                "",
                "<p>Tenki asks once whether to use your location. GeoClue is asked at city accuracy, and positions go to Open-Meteo rounded to 0.01°. Tenki opens on its cache and says how old it is, “Updated 12 min ago”, or that it's offline.</p>",
            );
            b += "</div>";
        }
        "tansu" => {
            b += r#"<div class="tiles">"#;
            b += &win(
                "map",
                "The map",
                "span-2",
                "<p>Pick a disk and see its partitions as a bar, each segment sized by its bytes and coloured by its filesystem, the used space shaded, above a table of every partition: name, filesystem, label, mount point, size, used and flags. SMART says how the disk is.</p>",
            );
            b += &win(
                "queue",
                "Nothing until you say so",
                "",
                "<p>Create, delete, format, resize, label, set flags, check, mount and unmount, make a new partition table, unlock LUKS: each joins a queue, and nothing happens until a confirmation lists them in plain words. A size that doesn't fit stays in its dialog with the reason.</p>",
            );
            b += &win(
                "careful",
                "Careful with your data",
                "",
                "<p>Resizing ext2, ext3 or ext4 checks and repairs the filesystem first, and a failed check stops before the partition changes. Disk images attach as loop devices, read-only if you like, and stay until you detach them. Everything goes through UDisks2.</p>",
            );
            b += "</div>";
        }
        "kura" => {
            b += &site.shot(
                "kura.png",
                "Kura's rings for a small home folder of 7.72 MB, Pictures and Downloads the largest, beside its Suggestions: four old Nix generations, low risk, with Open Yukimi's Storage.",
                &format!("Kura scanning the session's scratch home. {WALKTHROUGH}"),
                Frame::Lead,
            )?;
            b += r#"<div class="tiles">"#;
            b += &win(
                "rings",
                "The rings",
                "span-2",
                "<p>Scan a disk or a folder and see it as a sunburst of five rings, coloured by the dye ramp, the largest items labelled, with the folder's list beside it, largest first. Hover a segment to name it; click to go in, the middle to come back up. A scan runs on a thread per processor: about half a second for a home folder of 900,000 files with a warm cache.</p>",
            );
            b += &win(
                "collect",
                "The Collector and suggestions",
                "",
                "<p>Drag segments or rows onto the Collector, or press ⟦❖ K⟧; it totals what it holds and moves it all to the Trash when you say so. Suggestions find caches, build outputs, logs, old downloads and duplicates, with what each frees, its risk (safe, low risk, your call) and what it is in plain words. Every action asks first.</p>",
            );
            b += &win(
                "nix",
                "Nix-aware, agent-ready",
                "",
                "<p>The Nix store shows with its size and Open Yukimi's Storage; Kura never deletes store paths itself. Every scan is also written for agents as <code>index.json</code>, with a readable <code>index.md</code>, and <code>aizome-kura scan --json</code>, <code>report</code>, <code>hints</code> and <code>trash</code> work from a terminal.</p>",
            );
            b += "</div>";
        }
        "myaku" => {
            b += r#"<div class="tiles">"#;
            b += &win(
                "apps",
                "Apps, not just processes",
                "span-2",
                "<p>Each app shows with all its processes together, a browser's helpers or a terminal and what runs in it, by processor, memory, energy, disk and graphics use, with graphs over time. Sort, search, quit, force quit, pause and resume. Groups follow windows and process trees: an app launched from a terminal starts a group of its own, and the inspector's Grouping section explains why each process is where it is.</p>",
            );
            b += &win(
                "gpu",
                "Every graphics card",
                "",
                "<p>AMD and Intel through the kernel's DRM statistics, NVIDIA through NVML, loaded only where there's an NVIDIA card. With several cards a Card column says which each app uses, and the table can show one card's clients alone. Each card's graph sits in the strip along the bottom: how busy, how full.</p>",
            );
            b += &win(
                "energy",
                "Energy, shared out fairly",
                "",
                "<p>The processor's meter is read for you by a small helper that runs as root with no capabilities, starts when asked and leaves after a minute without a request. Each card's meter adds its share, and Energy Meters… shows every meter and which readings count toward the total.</p>",
            );
            b += "</div>";
        }
        "nisshi" => {
            b += &site.shot(
                "nisshi-night.png",
                "Nisshi in Night on All Messages: a timeline of the boot, a wpa_supplicant disconnect selected with every journal field below it, and suggestion chips for its problems, connections and roaming.",
                NISSHI,
                Frame::Lead,
            )?;
            b += r#"<div class="tiles">"#;
            b += &win(
                "places",
                "Places and search",
                "span-2",
                "<p>All Messages, Errors &amp; Warnings, My Session, Kernel, Crashes and Logins &amp; Security, each counted; then the sources that wrote the most, and the log files under <code>/var/log</code>. Search as you type with tokens: <code>app:</code>, <code>unit:</code>, <code>pid:</code>, <code>level:</code>, <code>since:</code> and <code>until:</code> (<code>10m</code>, <code>yesterday</code>), <code>/regex/</code>, <code>-word</code> and <code>\"a phrase\"</code>. While a key's value is being typed, the strip offers the values there are.</p>",
            );
            b += &win(
                "suggest",
                "Suggestions",
                "",
                "<p>With nothing selected, the strip says what stands out: errors and who wrote them, failed units, crashes, out-of-memory kills. With a message selected, it offers its source, its problems, the topics Nisshi knows that program writes about (21 kinds of program, from NetworkManager to sshd) and messages like it, to show or hide.</p>",
            );
            b += &win(
                "timeline",
                "The timeline",
                "",
                "<p>Messages over the boot, errors and warnings at the foot of each bar, on a square-root scale so a few errors show beside thousands of messages. Drag across it to see that stretch. Around This Time shows a minute either side of any message.</p>",
            );
            b += &win(
                "live",
                "Live, and yours",
                "span-2",
                "<p>New messages arrive as they're written. Programs' colours show in the theme's; launched apps are named after the app, each launch on its own. Your own session's journal is open to you, and Unlock… asks once for an administrator's password to read the system's, until Lock or the window closes.</p>",
            );
            b += "</div>";
            b += &site.shot(
                "nisshi-day.png",
                "Nisshi in Day searching for kex: 34 sshd errors with the matching text highlighted, and the timeline showing when they came.",
                NISSHI,
                Frame::Window,
            )?;
        }
        "shashin" => {
            b += r#"<div class="tiles">"#;
            b += &win(
                "view",
                "Viewing",
                "span-2",
                "<p>PNG, JPEG, GIF (animated), WebP, BMP, TIFF and ICO, and SVG rendered at the zoom. Pictures fit the window and are never enlarged past 100%. Zoom about the pointer with ⟦❖ +⟧ and ⟦❖ −⟧, scrolling or pinching; ⟦❖ 0⟧ is 100%, a picture pixel to a screen pixel, and ⟦❖ 9⟧ fits. ← and → step through the folder in Hako's order, and F shows a filmstrip.</p>",
            );
            b += &win(
                "edit",
                "Turning and more",
                "",
                "<p>Rotate with ⟦❖ L⟧ and ⟦❖ R⟧, then save: a JPEG's orientation is rewritten without re-encoding a pixel. Set as Wallpaper for Night, Day or both. Get Info (⟦❖ I⟧) gives the camera, lens and settings. Full screen, and a slideshow with ⟦⇧ ❖ F⟧.</p>",
            );
            b += &win(
                "speed",
                "Quick",
                "",
                "<p>Pictures decode on the kit's workers, a camera's own thumbnail shows first, and the next and previous pictures are read ahead. Every frame at rest is drawn for exactly that zoom.</p>",
            );
            b += "</div>";
        }
        "shiori" => {
            b += r#"<div class="tiles">"#;
            b += &win(
                "read",
                "Reading",
                "span-2",
                "<p>hayro, a PDF renderer in Rust, draws the pages, in a process of its own for each document. The pages sit in one column, fitted to the window's width at first; ⟦❖ 8⟧ fits the width, ⟦❖ 9⟧ the page, ⟦❖ 0⟧ is 100%. Go to Page… is ⟦⌥ ❖ G⟧. The sidebar (⟦⌥ ❖ S⟧) shows each page small, or the document's outline, marking where you are.</p>",
            );
            b += &win(
                "text",
                "Text and links",
                "",
                "<p>Drag to select across pages, double-click a word, triple-click a line. Find (⟦❖ F⟧) searches the whole document from the page you're on. Links within the document go to their place; web and mail links open in their apps, and a document can't start programs.</p>",
            );
            b += &win(
                "place",
                "Your place",
                "",
                "<p>Each document opens where you left it, at the same zoom and with the same sidebar. A file that changes while it's open, a LaTeX build or a download finishing, is read again where you are.</p>",
            );
            b += &win(
                "robust",
                "Sturdy and fast",
                "span-2",
                "<p>A page that trips the renderer costs that page, not the window: Shiori starts another worker and carries on. Pages are drawn in tiles for exactly the zoom and the screen's scale, and the pages above and below are drawn ahead.</p>",
            );
            b += "</div>";
        }
        "utsushi" => {
            b += r#"<div class="tiles">"#;
            b += &win(
                "plays",
                "Plays what FFmpeg reads",
                "span-2",
                "<p>MP4, Matroska, WebM, QuickTime, AVI, MPEG transport streams, MP3, AAC, FLAC, Ogg, Opus, WAV and the rest. Folders play in Hako's order. A song shows its cover, its title, and who made it on what album when.</p>",
            );
            b += &win(
                "light",
                "Light by design",
                "",
                "<p>Video decodes on the GPU through VA-API (H.264, HEVC, VP9 and AV1 on recent Intel and AMD), and each frame goes to the compositor as the decoder left it, never copied or converted. Out of sight, nothing decodes while the sound plays on. Audio decodes ahead in bursts: about one wakeup a second.</p>",
            );
            b += &win(
                "measured",
                "Measured",
                "",
                r#"<p>A 1080p H.264 film for 30 seconds, sound off, on an Intel Core Ultra 7 365:</p>
<table class="numbers"><thead><tr><th scope="col">Player</th><th scope="col">CPU</th><th scope="col">Switches a second</th></tr></thead><tbody>
<tr><th scope="row">Utsushi</th><td>1.2%</td><td>115</td></tr>
<tr><th scope="row">mpv, VA-API to dmabuf</th><td>2.2%</td><td>405</td></tr>
<tr><th scope="row">mpv, VA-API to gpu-next</th><td>3.5%</td><td>376</td></tr>
<tr><th scope="row">mpv with Sekka's mpv.conf</th><td>11.3%</td><td>401</td></tr>
</tbody></table>"#,
            );
            b += &win(
                "keys",
                "Keys and subtitles",
                "span-2",
                "<p>⟦Space⟧ or ⟦K⟧ plays and pauses; ⟦←⟧ ⟦→⟧ move five seconds and ⟦J⟧ ⟦L⟧ ten; ⟦↑⟧ ⟦↓⟧ change the volume and ⟦M⟧ mutes; ⟦[⟧ ⟦]⟧ slow down and speed up with the pitch kept; ⟦,⟧ ⟦.⟧ step a frame; ⟦F⟧ is full screen and ⟦S⟧ saves the picture. Subtitles embedded in the file, or beside it with the film's name, show by themselves; ⟦C⟧ picks the next. A film over five minutes opens where you left it, and the menu bar's media controls and your media keys work with it.</p>",
            );
            b += "</div>";
        }
        "koe" => {
            b += &site.shot(
                "koe.png",
                "Koe's Dictation page: Dictation is off, with Download a Model and Turn On (Base, 148 MB); six speech models, each with Download; and the recognition options.",
                "The installed Koe under a private headless Aizome compositor at 1280 × 800, captured with grim on 29 October 2026 for Sekka's application-startup check, with a fresh test profile and no models. The bar names its temporary desktop entry, Startup Window.",
                Frame::Lead,
            )?;
            b += r#"<div class="tiles">"#;
            b += &win(
                "dictate",
                "Dictation",
                "span-2",
                "<p>Press ⟦❖ ⌥ D⟧ in any app, speak, and press it again: what you said is typed where you were. Koe recognises speech on this computer with Whisper, on the graphics card through Vulkan where there is one.</p>",
            );
            b += &win(
                "setup",
                "Set up without a terminal",
                "",
                "<p>Download a Model and Turn On fetches Base (148 MB), checks it against a pinned SHA-256 and turns dictation on with its shortcut. Nothing is downloaded until you ask. Pick a model (Tiny, Base or Small, multilingual or English-only), the language, translation into English, the graphics card, and whether to finish after a pause.</p>",
            );
            b += &win(
                "words",
                "Your words",
                "",
                "<p>Vocabulary and Teaching keeps packs of words, hints and approved corrections, shareable as <code>.koe.json</code>. Audio and transcripts stay in memory, and Koe checks the focus and the lock before it types.</p>",
            );
            b += "</div>";
        }
        "annai" => {
            b += r#"<div class="tiles">"#;
            b += &win(
                "tour",
                "The tour",
                "span-2",
                "<p>Annai greets you at your first login and is there again from Help › Aizome Tour: short pages that make Aizome's ideas plain, each with a small live illustration drawn in Aizome's look. Welcome, with the mark and the name ⟪藍染⟫; Tabs; the menu bar and the menu at the pointer; the palette; the Toolchest; Desks and the Deskbar; Super as the command key; and Make it yours, with buttons that open Settei's Appearance, Displays, Network and Keyboard.</p>",
            );
            b += &win(
                "step",
                "Step through",
                "",
                "<p>⟦←⟧ ⟦→⟧ and ⟦↩⟧ step through the pages. Show at login brings it back next time.</p>",
            );
            b += "</div>";
        }
        other => anyhow::bail!("no page for the app {other}"),
    }
    Ok(b)
}
