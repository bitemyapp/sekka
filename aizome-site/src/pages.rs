//! What the site says. Every claim here comes from the repository's
//! READMEs and docs: desktop/aizome/README.md, desktop/aizome/docs (shell.md,
//! architecture.md, compositor.md, performance.md, rendering.md,
//! greeter.md, keep-awake.md, recovery.md, install.md), the root README.md,
//! DEVELOPERS.md and docs/screenshots.md, and the theme crate the colours
//! and metrics are read from. Keep it so: when a feature changes, change
//! this with it.

use crate::apps::{self, APPS, VM, WALKTHROUGH, GROUP};
use crate::html::{Menu, Page, SEKKA, SEKKA_DOWNLOAD, SOURCE, esc, mark};
use crate::theme::PAIRS;
use crate::{Frame, Site, win};
use anyhow::Result;
use std::fmt::Write;

/// A page's opening window: its tab names the page, and the frame holds
/// the page's one h1.
fn head(tab: &str, body: &str, class: &str) -> String {
    format!(
        r#"<section class="window focus page-head {class}"><div class="tab" aria-hidden="true">{}</div><div class="frame">{body}</div></section>"#,
        esc(tab)
    )
}

const NIXOS_FLAKE: &str = r#"{
  inputs.sekka.url = "github:bitemyapp/sekka";
  outputs = { nixpkgs, sekka, ... }: {
    nixosConfigurations.example = nixpkgs.lib.nixosSystem {
      modules = [
        sekka.nixosModules.aizome
        { programs.aizome.enable = true; }
      ];
    };
  };
}"#;

/// Sekka's binary cache, as installer/genkan/rust/system/binary-cache.nix
/// configures it on Sekka's own systems.
const NIXOS_CACHE: &str = r#"nix.settings = {
  extra-substituters = [ "https://cache.sekka.computer" ];
  extra-trusted-public-keys = [
    "cache.sekka.computer-1:JJsOkmRt3iCwjvJrol8x2a7euU+kMV7iHaDSFzofA+Y="
  ];
};"#;

pub fn home(site: &Site) -> Result<Page> {
    let mut b = String::new();
    write!(
        b,
        r#"<section class="window focus hero" aria-labelledby="hero-title">
  <div class="tab" aria-hidden="true">Aizome</div>
  <div class="frame">
    <p class="lockup" aria-label="Aizome 藍染">{mark}<span class="wordmark" aria-hidden="true">aizome</span>{seal}</p>
    <h1 id="hero-title">The desktop Sekka is made for.</h1>
    <p class="lead">Aizome ⟪藍染⟫ is indigo dyeing: cloth dipped again and again, each dip adding depth. It is also a Wayland desktop written in Rust on its own toolkit, Sukumo ⟪蒅⟫, from the compositor to the calculator, with its ideas taken from Haiku, NeXTSTEP and IRIX.</p>
    <p class="actions"><a class="button primary" href="{SEKKA_DOWNLOAD}">Get it with Sekka</a><a class="button" href="/desktop/">Tour the desktop</a></p>
  </div>
</section>
"#,
        mark = mark("hero-mark", None),
        seal = site.ja_vertical("藍染", "hero-seal")?,
    )?;
    b += &site.group(
        "tour",
        "Aizome, three ways",
        &[
            (
                "The desktop",
                site.shot(
                    "desktop.png",
                    "The whole Aizome desktop in Night: the menu bar along the top with Sumi's menus and four Desks; Sumi editing a Markdown file; Files and Hotaru, Aizome's terminal, running fastfetch, stacked in one tab group; the Deskbar at the top right with each app's windows and Tenki's notification filed under it.",
                    &format!("The whole desktop: Sumi, then Files and Hotaru stacked in one group, and a notification filed under Tenki in the Deskbar. {VM}"),
                    Frame::BareLead,
                )?,
            ),
            (
                "The menu at the pointer",
                site.shot(
                    "menu.png",
                    "The same desktop with Sumi's whole menu open at the pointer: Info, File, Edit, Format, View, Window, Help and Services, then Hide and Quit.",
                    &format!("⟦❖⟧-right-click anywhere opens the focused app's whole menu at the pointer. {VM}"),
                    Frame::Bare,
                )?,
            ),
            (
                "The palette",
                site.shot(
                    "palette.png",
                    "The command palette over the desktop with “find” typed: Sumi's Find commands with their menu paths and shortcuts, matching settings below, and Find's shortcut as keycaps on the right.",
                    &format!("⟦❖ Space⟧ with “find” typed: the focused app's menu commands, with their shortcuts, and settings. {VM}"),
                    Frame::Bare,
                )?,
            ),
        ],
    )?;

    b += r#"<p class="floor-title">What makes it Aizome</p><div class="tiles ideas">"#;
    for (id, title, text, link, label) in [
        (
            "idea-tabs",
            "Haiku's tabs",
            "Every window wears a tab only as wide as its title, amber when focused. Tabs slide along the top edge, and dropping one tab among another window's stacks them into a group, from any apps. A new window of the app you're using opens as another tab in its group.",
            "/desktop/#tabs",
            "Tabs and groups",
        ),
        (
            "idea-menus",
            "NeXT's menus",
            "One menu bar for the focused app, and the whole of its menu at the pointer on ⟦❖⟧-right-click. Any menu tears off into a palette that stays.",
            "/desktop/#menus",
            "Menus",
        ),
        (
            "idea-palette",
            "A palette for everything",
            "⟦❖ Space⟧ finds every menu command of the focused app, with its shortcut taught back to you, plus windows, files, apps, settings and arithmetic.",
            "/desktop/#palette",
            "The palette",
        ),
        (
            "idea-toolchest",
            "Toolchest, Deskbar and Desks",
            "IRIX's Toolchest and Desks, and a Deskbar from Haiku: every app with its windows, its notifications filed beneath it, and the Shelf, where a drag can be parked halfway.",
            "/desktop/#toolchest",
            "Toolchest and Deskbar",
        ),
        (
            "idea-super",
            "Super is the command key",
            "⟦❖ C⟧, ⟦❖ V⟧, ⟦❖ Z⟧ in every app, leaving Control to terminals and Emacs, and on every keyboard where your thumb finds it: press the key beside a new keyboard's space bar and Aizome suggests where Super goes on that keyboard alone.",
            "/desktop/#keyboard",
            "The keyboard",
        ),
        (
            "idea-chooser",
            "One file chooser",
            "Browsers, Electron apps, Zed and Flatpaks open and save through Files in miniature, over their own window, with their filters and options.",
            "/desktop/#chooser",
            "The file chooser",
        ),
        (
            "idea-fast",
            "Fast by construction",
            "Integer scaling, opaque surfaces, redraws of only what changed, and a 4 ms budget for every frame of the desktop and its apps. The GPU composites, with a software fallback that looks the same.",
            "/design/#fast",
            "The numbers",
        ),
    ] {
        b += &win(
            id,
            title,
            "",
            &format!(r#"<p>{text}</p><p><a href="{link}">{label}</a></p>"#),
        );
    }
    b += "</div>";

    let mut tiles = String::new();
    for app in &APPS {
        write!(
            tiles,
            r#"<li><a class="app-tile" href="{path}">{icon}<span class="app-tile-name">{name}</span><span class="app-tile-generic">{generic}</span></a></li>"#,
            path = app.path(),
            icon = site.icon(&app.id(), "app-tile-icon", 48)?,
            name = app.name,
            generic = app.generic,
        )?;
    }
    b += &win(
        "apps",
        "Its apps",
        "",
        &format!(
            r#"<p class="headline">From Settings to the calculator, every app is Aizome's own: one look, one set of keys, Night and Day.</p><ul class="app-grid">{tiles}</ul><p><a href="/apps/">What each app does</a></p>"#
        ),
    );

    write!(
        b,
        r#"<div class="tiles">
{sekka}
{nixos}
</div>
{sukumo}"#,
        sekka = win(
            "get-sekka",
            "Get it with Sekka",
            "",
            &format!(
                r#"<p>Aizome is the desktop Sekka is made for, selected by default in Genkan, its installer, which also offers Tatami, Plasma, GNOME, Hyprland and Xfce, instead or alongside. Download Sekka, start it, and the live session is Aizome with Genkan open. Installing erases the disk you choose.</p><p class="actions"><a class="button primary" href="{SEKKA_DOWNLOAD}">Download Sekka</a><a class="button" href="{SEKKA}">sekka.computer</a></p>"#
            ),
        ),
        nixos = win(
            "get-nixos",
            "Or on any NixOS",
            "",
            r#"<p>Add Sekka's flake and its Aizome module to a NixOS system with flakes, then pick Aizome on the login screen.</p><p><a href="/install/">How to install it</a></p>"#,
        ),
        sukumo = win(
            "sukumo",
            "Sukumo",
            "",
            "<p class=\"headline\">Sukumo ⟪蒅⟫ is the composted indigo leaf an indigo vat is built from. You never see it in the finished cloth.</p><p>It is Aizome's toolkit: the Wayland engine and native Rust toolkit every Aizome program draws with, from the compositor's own surfaces to the calculator, with its text engine, keyboards, clipboard and drag and drop. Text in a window's tab and text in the window come from the same engine.</p>",
        ),
    )?;
    Ok(Page {
        path: "/".into(),
        title: "Aizome: the desktop Sekka is made for".into(),
        description: "Aizome is a Wayland desktop written in Rust on its own toolkit, from the compositor to the calculator, with ideas from Haiku, NeXTSTEP and IRIX.".into(),
        menu: Menu::Home,
        body: b,
    })
}

/// A text window beside a picture, as two windows on a desk.
fn step(text: &str, shot: &str) -> String {
    format!(r#"<div class="step">{text}{shot}</div>"#)
}

pub fn desktop(site: &Site) -> Result<Page> {
    let mut b = head(
        "Desktop",
        "<p class=\"eyebrow\">The desktop</p><h1>Ideas from Haiku, NeXTSTEP and IRIX, made to work together.</h1><p class=\"lead\">One process, the shell, draws everything of Aizome that isn't a window or an app: the menu bar, the Toolchest, the Desks, the Deskbar and the Shelf, the palette, menus, notifications, the desktop and the lock screen. The compositor draws the tabs and frames. Here is what they do.</p>",
        "",
    );
    let sections = [
        ("tabs", "Tabs"),
        ("menus", "Menus"),
        ("palette", "Palette"),
        ("toolchest", "Toolchest"),
        ("deskbar", "Deskbar"),
        ("desks", "Desks"),
        ("keyboard", "Keyboard"),
        ("chooser", "Files"),
        ("notifications", "Notices"),
        ("capture", "Capture"),
        ("lock", "Lock"),
        ("recovery", "Recovery"),
    ];
    b += r#"<nav class="desks" aria-label="On this page"><ol>"#;
    for (i, (id, label)) in sections.iter().enumerate() {
        write!(
            b,
            r##"<li><a href="#{id}"><span class="desk-n" aria-hidden="true">{}</span>{label}</a></li>"##,
            i + 1
        )?;
    }
    b += "</ol></nav>";

    b += &win(
        "tabs",
        "Tabs and groups",
        "",
        "<p class=\"headline\">Every window wears a tab only as wide as its title, amber when it has the focus.</p><div class=\"cols\"><p>Tabs slide along the top edge with ⟦⇧⟧-drag, so overlapping windows stay easy to find. Drop one tab among another window's and the windows stack into a group, from any apps, the tab going where you dropped it; click a tab to show its window. Drag a background tab away to take it out, or ⟦⌥⟧-drag the one showing. A new window of the app you're using opens as another tab in its group, and Settei can turn that off.</p><p>⟦⌃ ❖ S⟧ stacks a window with the one behind it, and ⟦⌃ ⇧ ❖ S⟧ takes its tab out. ⟦⇧ ❖ [⟧ and ⟦⇧ ❖ ]⟧ show the previous and next tab. A double click on a tab, or its zoom box, fills the screen; a middle click sends the window to the back. Apps reopen where they last were.</p></div>",
    );
    b += &site.group(
        "stack",
        "Two apps in one tab group",
        &[
            (
                "Soroban showing",
                site.shot(
                    "group-soroban.png",
                    "Soroban and Google Chrome stacked in one group: Soroban's tab amber and its window showing, Chrome's tab idle beside it.",
                    &format!("Soroban showing. {GROUP}"),
                    Frame::BareLead,
                )?,
            ),
            (
                "Chrome showing",
                site.shot(
                    "group-chrome.png",
                    "The same group with Google Chrome's tab amber and its window showing, Soroban's tab idle beside it.",
                    &format!("A click on Chrome's tab shows it. {GROUP}"),
                    Frame::Bare,
                )?,
            ),
            (
                "Torn apart",
                site.shot(
                    "group-detached.png",
                    "Chrome dragged out of the group into a window of its own, over Soroban, which keeps its own tab.",
                    &format!("⟦⌥⟧-dragging the showing tab tears its window out again. {GROUP}"),
                    Frame::Bare,
                )?,
            ),
        ],
    )?;
    b += &step(
        &win(
            "marking",
            "The marking menu",
            "",
            "<p>Right-press a tab and its window's verbs ring it: fill, park, split left and right, close, stack, float, and to a Desk. ⟦⌃ ❖ Space⟧ opens it around the focused window from the keyboard: an arrow picks the verb that way, a letter one of the others, and ⟦D⟧ then a digit sends the window to a Desk.</p><p>The four arrows with ⟦⌥ ❖⟧ do the same without the menu: ⟦⌥ ❖ ↑⟧ fills, ⟦⌥ ❖ ↓⟧ parks, ⟦⌥ ❖ ←⟧ and ⟦⌥ ❖ →⟧ split.</p>",
        ),
        &site.shot(
            "marking-menu.png",
            "Sumi's window with its marking menu around the tab: fill above, split left and right, float, to Desk, close, stack and park.",
            &format!("A right press and pause on Sumi's tab. {WALKTHROUGH}"),
            Frame::Window,
        )?,
    );

    b += &step(
        &win(
            "menus",
            "Menus",
            "",
            "<p class=\"headline\">One menu bar, and the whole menu wherever the pointer is.</p><p>The menu bar shows the focused app's name in bold and its menus. ⟦❖⟧-right-click anywhere, or a right click on the empty desktop, opens the app's whole menu at the pointer, each menu a submenu, as NeXT did.</p><p>Drag any menu's title strip and it tears off into a palette that stays, moved by its strip, with a close box and a pin. Palettes show while their app has the focus, or always once pinned, and come back at your next login.</p><p>Menus forgive: moving toward an open submenu doesn't close it, typing filters a menu and its submenus, and ⟦F10⟧ or ⟦⌥ ❖ M⟧ takes the keyboard to the menu bar. Services act on the selected text in any app: a new note, open in Sumi, search the web, calculate.</p>",
        ),
        &site.shot(
            "walkthrough-menus.png",
            "Sumi's whole menu at the pointer with Edit open, the Toolchest torn off at the bottom left and Sumi's Format menu torn off at the right, each with a title strip and a pin.",
            &format!("The menu at the pointer, with the Toolchest and Format menu torn off into palettes. {WALKTHROUGH}"),
            Frame::Window,
        )?,
    );

    b += &step(
        &win(
            "palette",
            "The command palette",
            "",
            "<p class=\"headline\">⟦❖ Space⟧ searches every command of the app you're in.</p><p>Each menu item of the focused app, with its path and its shortcut in keycaps; windows; files in your home folder; apps; every setting, which opens Settei at that place; and actions: lock, sleep, Night or Day, Do Not Disturb, a screenshot, a recording. Type a sum and the first row is the answer.</p><p>Run a command from the menus or the palette three times in a week and the palette's coach shows you its keys. ⟦⇥⟧ moves between scopes, ⟦❖ K⟧ opens a row's actions, and what you pick is remembered, so the palette learns. It is drawn ahead while hidden, so it opens at once.</p>",
        ),
        &site.shot(
            "walkthrough-palette.png",
            "The palette over Sumi with “find” typed and Find and Replace selected; its detail pane shows the shortcut as keycaps and says it was run from menus three times this week.",
            &format!("The palette's shortcut coach for Find and Replace, which had been run from the palette three times. {WALKTHROUGH}"),
            Frame::Window,
        )?,
    );

    b += &step(
        &win(
            "toolchest",
            "The Toolchest",
            "",
            "<p class=\"headline\">IRIX's launcher, at the left of the menu bar.</p><p>The Toolchest button, or ⟦❖ ⌥ Space⟧, opens it: Applications by category, each with its icon, name and what it is; Places; Desks; Find, which opens the palette in one scope; System; and Session, to lock, sleep, restart, shut down or log out. Start typing and it becomes the palette.</p><p>Opening an app that's running brings its last window forward, from another Desk if need be. Application Launches… keeps the last 64 launches; one that fails says so, and Open in Logs shows its output in Nisshi.</p>",
        ),
        &site.shot(
            "day.png",
            "Aizome in Day: the Toolchest open at the top left with Applications and its categories, over Sumi and foot, the Deskbar at the right.",
            &format!("Day, switched live from Settings, with the Toolchest's Applications open. {WALKTHROUGH}"),
            Frame::Window,
        )?,
    );

    b += &step(
        &win(
            "deskbar",
            "The Deskbar and the Shelf",
            "",
            "<p class=\"headline\">Haiku's Deskbar instead of a Dock.</p><p>At the top right, each running app with its windows beneath it: the focused one marked in the focus colour, hidden ones in italics, stacked ones and those on another Desk said so. Notifications are filed under the app that sent them. Drag a window's row onto a Desk in the menu bar to move it there.</p><p>The Shelf, from NeXT, parks a drag halfway: files, folders, text, images and links wait there until you drag them on, across sessions. Drag the Deskbar's grip to stand it at the left or the right, and its inner edge to make it wider, slimmer or icons only. It can hide itself until you need it.</p>",
        ),
        &site.shot(
            "walkthrough-desktop.png",
            "Night: Sumi with text selected, foot and Files stacked as one group, three notifications from Mail filed in the Deskbar, README.md and docs parked on the Shelf, and icons on the desktop.",
            &format!("The Deskbar with Mail's notifications filed under it and two items parked on the Shelf. {WALKTHROUGH}"),
            Frame::Window,
        )?,
    );

    b += &win(
        "desks",
        "Desks",
        "",
        "<p class=\"headline\">Named Desks keep work apart, a Desk to each display.</p><div class=\"cols\"><p>Four Desks to start, named in the menu bar's strip, each with a folder of its own where the terminal opens and the file chooser starts. Each display shows a Desk of its own: ⟦⌃ ❖ 1⟧ to ⟦⌃ ❖ 9⟧ show one on the display you're using, ⟦⌃ ❖ ←⟧ and ⟦⌃ ❖ →⟧ step, and ⇧ added takes the window along.</p><p>⟦⌃ ❖ ↑⟧ shows every Desk with its windows; drag a window to another Desk. ⟦❖ ⇥⟧ switches apps, then that app's windows. A window moved to a display of another size keeps its place in proportion, and a display that comes back gets its Desk back.</p></div>",
    );
    b += r#"<div class="pair">"#;
    b += &site.shot(
        "overview.png",
        "The Desks overview: four Desks, Mail, Build, Write and Notes, with their windows small, the current one outlined in amber, and a slot for a new Desk.",
        &format!("⟦⌃ ❖ ↑⟧: four Desks with their windows. {WALKTHROUGH}"),
        Frame::Window,
    )?;
    b += &site.shot(
        "switcher.png",
        "The switcher over the desktop: a row of apps, Sumi, Soroban, Fusen, Yukimi, Tokei, Koyomi, Foot and Hako, one selected with its window below as a live thumbnail.",
        &format!("⟦❖ ⇥⟧: the apps, then the selected app's windows with a live thumbnail. {WALKTHROUGH}"),
        Frame::Window,
    )?;
    b += "</div>";

    b += &win(
        "keyboard",
        "Super, the command key",
        "",
        "<p class=\"headline\">⟦❖ C⟧, ⟦❖ V⟧, ⟦❖ Z⟧: Super does what ⌘ does on a Mac, in every app.</p><div class=\"cols\"><p>Control stays free for terminals and Emacs. Apps that aren't built with Aizome's kit still answer: the compositor turns ⟦❖ C⟧ into the Control shortcut they expect, and into Control-Shift-C in a terminal. The desktop keeps only a handful of chords for itself, all listed on the <a href=\"/keys/\">keys page</a>.</p><p>Each keyboard keeps its own modifier keys, Caps Lock and Compose key. Plug in or pair a keyboard and Aizome offers to set it up: press the key beside its space bar and Settei suggests where Super goes, so it sits under your thumb on a Mac keyboard and a PC keyboard alike. Layouts and shortcuts are the same on every keyboard.</p></div>",
    );

    b += &win(
        "chooser",
        "One file chooser",
        "",
        "<p class=\"headline\">Every app that asks the desktop to open or save a file gets Files in miniature.</p><div class=\"cols\"><p>Chromium, Chrome and Electron apps such as VS Code, Obsidian, Slack and Signal; Zed; Flatpaks; Firefox and GTK 3 apps. The chooser opens over the app's own window as its dialog, with Hako's sidebar, its path bar, and the folder as a list sorted by any column, folders first.</p><p>A name you save as can be a path; a file that's there already is replaced only after asking. The app's filters and options show as pop-ups and checkboxes. The Go menu's keys are Hako's, and the chooser remembers where each app last chose, starting there next time, else in the current Desk's folder.</p></div>",
    );

    b += &step(
        &win(
            "notifications",
            "Notifications",
            "",
            "<p class=\"headline\">A line in the menu bar, then filed under the app that sent it.</p><p>A new notification shows in the menu bar, the app's glyph, summary and the first line, in place of the menus for five seconds, then goes to the Deskbar under its app, with its actions as buttons. Critical ones stay as a card until you act. Do Not Disturb, from the palette, the clock or a schedule, files them quietly.</p><p>Volume, brightness and the keyboard's backlight show a level at the bottom of the screen as you press their keys.</p>",
        ),
        &site.shot(
            "recording-saved.png",
            "The menu bar saying Screen recording saved with the file's name, and in the Deskbar the recording's card with Show in Files and Open, above a screenshot's card with Show in Hako.",
            "A recording announced in the menu bar's line and filed under Screen Recording in the Deskbar. Taken on 18 October 2026 in a headless session at 2880 × 1800, scale 2, with aizomectl screenshot.",
            Frame::Window,
        )?,
    );

    b += &step(
        &win(
            "capture",
            "Screenshots, recordings and sharing",
            "",
            "<p class=\"headline\">⟦❖ ⇧ 5⟧ opens the Capture menu, as ⌘⇧5 does on a Mac.</p><p>⟦❖ ⇧ 3⟧ takes the screen, ⟦❖ ⇧ 4⟧ an area, and the Capture menu the window, recordings of the screen or an area, with what the computer plays and the microphone if you like, and your recordings. ⟦⌥ Print⟧ starts and stops a recording. While one runs, a red dot and its time lead the systray.</p><p>Recordings are encoded on the GPU from the display's own planes, at no cost to the compositor, and land in <code>~/Videos/Recordings</code>. When a browser shares your screen, a banner asks which screen or window, and a blue chip in the Deskbar says what's shared and stops it.</p>",
        ),
        &site.shot(
            "recording.png",
            "A recording under way: a red dot and 0:04 at the front of the systray, and the Capture menu open with Stop Recording, three kinds of screenshot, Record the Screen, Record an Area and Open Recordings.",
            "Recording, with the Capture menu open. Taken on 18 October 2026 in a headless session at 2880 × 1800, scale 2, with aizomectl screenshot; the recorder was wf-recorder.",
            Frame::Window,
        )?,
    );

    b += &step(
        &win(
            "lock",
            "The lock screen and the login screen",
            "",
            "<p class=\"headline\">⟦⌃ ❖ Q⟧ locks; so does leaving the computer, and sleep.</p><p>Every display gets the whole lock screen: the wallpaper under a scrim of kachi, the darkest indigo, the time in IBM Plex Mono, your picture and name, and a password field with the focus rail. Caps Lock is pointed out. A display that comes back while locked gets a field of its own at once.</p><p>Aizome's login screen is drawn with the same pieces. It shows every session the computer has, Aizome, other desktops and X11 ones, the people as a row of pictures, and the session that will start; ⟦F2⟧ lists the sessions and ⟦F3⟧ opens Sleep, Restart and Shut Down.</p>",
        ),
        &site.shot(
            "lock.png",
            "The lock screen: the night wallpaper dimmed, the time 00:03 and the date, an initial on the selection colour, the user's name, and a password field with dots typed.",
            &format!("The lock screen with a password typed. {WALKTHROUGH}"),
            Frame::Window,
        )?,
    );

    b += &step(
        &win(
            "recovery",
            "Recovery",
            "",
            "<p class=\"headline\">⟦⌃ ⌥ Del⟧ gets a stuck desktop going again.</p><p>From anywhere, over full-screen games and the lock screen, the compositor draws its recovery overlay itself: the running apps with their processor and memory, those not responding first, with Close and Force Quit; and Reset Input, Restart Shell, Reload Aizome, Lock, Log Out and a text console. It works when the shell or an app is the problem.</p><p><strong>Keep Awake</strong> is its quieter cousin: a switch in the battery panel, a cup in the systray while it's on, and the computer stays awake through long builds and downloads, lid closed or not, until you turn it off. The cup turns red if the hold can't be confirmed.</p>",
        ),
        &site.shot(
            "recovery.png",
            "The recovery overlay: three apps with their processor and memory, the first marked Not Responding and selected, Close and Force Quit for it, and Reset Input, Restart Shell, Reload Aizome, Lock, Log Out and Text Console.",
            "The overlay at 2×, from Aizome's recovery notes (desktop/aizome/docs/recovery.md), taken before Settings was renamed Settei: it shows its old name, Shitsurae.",
            Frame::Window,
        )?,
    );

    Ok(Page {
        path: "/desktop/".into(),
        title: "The Aizome desktop: tabs, menus, the palette and more".into(),
        description: "Haiku's tabs, NeXT's menus, a command palette for everything, IRIX's Toolchest and Desks, Super as the command key, one file chooser.".into(),
        menu: Menu::Desktop,
        body: b,
    })
}

pub fn apps_index(site: &Site) -> Result<Page> {
    let mut b = head(
        "Apps",
        "<p class=\"eyebrow\">The apps</p><h1>Every app speaks Aizome.</h1><p class=\"lead\">Each app's name is a Japanese word that says what it is for, in keeping with Aizome and Sekka, and each carries a plain English name too, which the Toolchest, the palette and the Deskbar show beside it and search by. Every one is built with Aizome's kit: one look, Night and Day, every target 24 points or more, and all of it reachable by keyboard.</p>",
        "",
    );
    b += r#"<ul class="app-list">"#;
    for app in &APPS {
        write!(
            b,
            r#"<li class="window app-card"><div class="tab" aria-hidden="true">{name}</div><div class="frame"><a class="app-card-link" href="{path}">{icon}<span class="app-card-text"><span class="app-card-name">{name} {kanji}</span><span class="app-card-generic">{generic} · <span class="app-card-meaning">{meaning}</span></span><span class="app-card-summary">{summary}</span></span></a></div></li>"#,
            path = app.path(),
            icon = site.icon(&app.id(), "app-card-icon", 64)?,
            name = app.name,
            kanji = site.ja(app.kanji)?,
            generic = app.generic,
            meaning = esc(app.meaning),
            summary = esc(app.summary),
        )?;
    }
    b += "</ul>";
    b += &win(
        "session",
        "Part of the session",
        "",
        "<p>Not apps of their own, but Aizome too: the Toolchest, the command palette, notifications, the lock screen and the password dialog, all drawn by the shell; and the file chooser other apps open and save with. <a href=\"/desktop/\">The desktop</a></p>",
    );
    Ok(Page {
        path: "/apps/".into(),
        title: "Aizome's apps".into(),
        description: "Settei, Yukimi, Hako, Sumi, Hotaru, Fusen, Koyomi, Tokei, Soroban, Tenki, Tansu, Kura, Myaku, Nisshi, Shashin, Shiori, Utsushi, Koe and Annai.".into(),
        menu: Menu::Apps,
        body: b,
    })
}

fn deskbar(site: &Site, current: usize) -> Result<String> {
    let mut s = String::from(
        r#"<aside class="deskbar" aria-label="Every app"><div class="deskbar-grip" aria-hidden="true"></div><ul class="deskbar-apps">"#,
    );
    for (i, app) in APPS.iter().enumerate() {
        let cur = if i == current {
            r#" aria-current="page""#
        } else {
            ""
        };
        write!(
            s,
            r#"<li><a href="{path}"{cur}>{icon}<span class="db-name">{name}</span><span class="db-generic">{generic}</span></a></li>"#,
            path = app.path(),
            icon = site.icon(&app.id(), "db-icon", 16)?,
            name = app.name,
            generic = app.generic,
        )?;
    }
    write!(
        s,
        r#"</ul><div class="deskbar-note"><p class="db-head">{icon}<span class="db-name">Sekka</span><span class="db-new">1 new</span></p><a class="db-card" href="{SEKKA_DOWNLOAD}"><strong>Get Aizome</strong><span>Sekka's installer sets it all up.</span></a></div></aside>"#,
        icon = site.icon("org.aizome.Toolchest", "db-icon", 16)?,
    )?;
    Ok(s)
}

pub fn app(site: &Site, i: usize) -> Result<Page> {
    let app = &APPS[i];
    let mut content = format!(
        r#"<section class="window focus page-head app-head"><div class="tab" aria-hidden="true">{name}</div><div class="frame"><div class="app-head-row">{icon}<div><p class="eyebrow">{generic}</p><h1><span class="app-name">{name}</span> {kanji}</h1><p class="app-meaning"><em lang="ja">{name}</em> means {meaning}.</p></div></div><p class="lead">{headline}</p><dl class="app-facts"><div><dt>Generic name</dt><dd>{generic}</dd></div><div><dt>Command</dt><dd><code>aizome-{slug}</code></dd></div><div><dt>App ID</dt><dd><code>{id}</code></dd></div></dl></div></section>"#,
        name = app.name,
        icon = site.icon(&app.id(), "page-icon", 96)?,
        generic = app.generic,
        kanji = site.ja(app.kanji)?,
        meaning = esc(app.meaning),
        headline = esc(app.headline),
        slug = app.slug,
        id = app.id(),
    );
    content += &apps::body(site, app)?;
    let prev = &APPS[(i + APPS.len() - 1) % APPS.len()];
    let next = &APPS[(i + 1) % APPS.len()];
    write!(
        content,
        r#"<nav class="pager" aria-label="More apps"><a rel="prev" href="{pp}"><span aria-hidden="true">←</span> {pn} <span class="pager-generic">{pg}</span></a><a href="/apps/">Every app</a><a rel="next" href="{np}">{nn} <span class="pager-generic">{ng}</span> <span aria-hidden="true">→</span></a></nav>"#,
        pp = prev.path(),
        pn = prev.name,
        pg = prev.generic,
        np = next.path(),
        nn = next.name,
        ng = next.generic,
    )?;
    let body = format!(
        r#"<div class="with-deskbar"><div class="content">{content}</div>{}</div>"#,
        deskbar(site, i)?
    );
    Ok(Page {
        path: app.path(),
        title: format!("{} ({}): Aizome", app.name, app.generic),
        description: app.summary.to_owned(),
        menu: Menu::Apps,
        body,
    })
}

/// Rows of a key table: what, then the keys, as a menu lists them.
fn key_rows(rows: &[(&str, &str)]) -> String {
    let mut s = String::from(r#"<table class="keytable"><tbody>"#);
    for (what, keys) in rows {
        let _ = write!(s, r#"<tr><th scope="row">{what}</th><td>{keys}</td></tr>"#);
    }
    s.push_str("</tbody></table>");
    s
}

pub fn keys(_site: &Site) -> Result<Page> {
    let mut b = head(
        "Keys",
        "<p class=\"eyebrow\">Keys</p><h1>Super is the command key.</h1><p class=\"lead\">⟦❖ C⟧, ⟦❖ V⟧, ⟦❖ Z⟧, ⟦❖ Q⟧, ⟦❖ W⟧ and the rest belong to apps, as ⌘ does on a Mac. The desktop keeps only the chords below. ⟦⌃ ❖⟧ is the window manager's layer, and ⟦⇧⟧ added to it carries something along: a window to a Desk, a tab through its group.</p><dl class=\"legend\"><div><dt>⟦❖⟧</dt><dd>Super</dd></div><div><dt>⟦⇧⟧</dt><dd>Shift</dd></div><div><dt>⟦⌥⟧</dt><dd>Alt</dd></div><div><dt>⟦⌃⟧</dt><dd>Control</dd></div></dl>",
        "",
    );
    b += r#"<div class="tiles keys-tiles">"#;
    b += &win(
        "system",
        "Everywhere",
        "",
        &key_rows(&[
            ("The command palette", "⟦❖ Space⟧"),
            ("The Toolchest", "⟦❖ ⌥ Space⟧"),
            ("Switch apps, then their windows", "⟦❖ ⇥⟧ ⟦❖ ⇧ ⇥⟧"),
            ("The app's next window", "⟦❖ `⟧"),
            ("Hide the app, hide the others", "⟦❖ H⟧ ⟦⌥ ❖ H⟧"),
            ("Minimise the window", "⟦❖ M⟧"),
            ("Your terminal, Hotaru unless Settei names another, in the Desk's folder", "⟦❖ ↩⟧"),
            ("Lock", "⟦⌃ ❖ Q⟧"),
            ("Recovery: force quit, reset input, restart the shell", "⟦⌃ ⌥ Del⟧"),
            ("Reload what changed: the compositor's logic, the shell, the settings", "⟦⌃ ❖ R⟧"),
            ("The app's whole menu at the pointer", "⟦❖⟧-right-click"),
            ("Focus the menu bar", "⟦F10⟧ ⟦⌥ ❖ M⟧"),
        ]),
    );
    b += &win(
        "windows",
        "Windows",
        "",
        &key_rows(&[
            ("Fill, park", "⟦⌥ ❖ ↑⟧ ⟦⌥ ❖ ↓⟧"),
            ("Split left, split right", "⟦⌥ ❖ ←⟧ ⟦⌥ ❖ →⟧"),
            ("The marking menu of the focused window", "⟦⌃ ❖ Space⟧"),
            ("Stack with the window behind", "⟦⌃ ❖ S⟧"),
            ("Take the tab out of its group", "⟦⌃ ⇧ ❖ S⟧"),
            ("Previous tab, next tab", "⟦❖ ⇧ [⟧ ⟦❖ ⇧ ]⟧"),
            ("Move the tab left, right", "⟦⌃ ⇧ ❖ [⟧ ⟦⌃ ⇧ ❖ ]⟧"),
            ("Full screen", "⟦⌃ ❖ F⟧"),
            ("Move a window, resize it, from anywhere in it", "⟦⌃ ❖⟧-drag, ⟦⌃ ❖⟧-right-drag"),
        ]),
    );
    b += &win(
        "desks",
        "Desks",
        "",
        &key_rows(&[
            ("Show Desk 1 to 9 on this display", "⟦⌃ ❖ 1⟧ … ⟦⌃ ❖ 9⟧"),
            ("…and take the window along", "⟦⌃ ⇧ ❖ 1⟧ … ⟦⌃ ⇧ ❖ 9⟧"),
            ("Previous Desk, next Desk", "⟦⌃ ❖ ←⟧ ⟦⌃ ❖ →⟧"),
            ("…and take the window along", "⟦⌃ ⇧ ❖ ←⟧ ⟦⌃ ⇧ ❖ →⟧"),
            ("Every Desk at once", "⟦⌃ ❖ ↑⟧"),
        ]),
    );
    b += &win(
        "capture",
        "Capture",
        "",
        &key_rows(&[
            ("Screenshot of the screen", "⟦❖ ⇧ 3⟧"),
            ("Screenshot of an area", "⟦❖ ⇧ 4⟧ ⟦Print⟧"),
            ("The Capture menu: the window, recordings", "⟦❖ ⇧ 5⟧"),
            ("Record an area or a display; again to stop", "⟦⌥ Print⟧"),
            ("Dictation, with Koe turned on", "⟦❖ ⌥ D⟧"),
        ]),
    );
    b += &win(
        "menu-keys",
        "In menus and the palette",
        "",
        &key_rows(&[
            ("Move", "⟦↑⟧ ⟦↓⟧"),
            ("Open a submenu, close it", "⟦→⟧ ⟦←⟧"),
            ("Choose", "⟦↩⟧"),
            ("Filter the menu", "type"),
            ("Clear, then close", "⟦⎋⟧"),
            ("The palette's next scope, previous", "⟦⇥⟧ ⟦⇧ ⇥⟧"),
            ("Run and keep the palette open", "⟦❖ ↩⟧"),
            ("A row's actions", "⟦❖ K⟧"),
        ]),
    );
    b += &win(
        "chooser",
        "In the file chooser",
        "",
        &key_rows(&[
            ("Back, forward", "⟦❖ [⟧ ⟦❖ ]⟧"),
            ("The enclosing folder", "⟦❖ ↑⟧"),
            ("Recent, Home, Desktop", "⟦⇧ ❖ F⟧ ⟦⇧ ❖ H⟧ ⟦⇧ ❖ D⟧"),
            ("Documents, Downloads", "⟦⇧ ❖ O⟧ ⟦⌥ ❖ L⟧"),
            ("Go to a folder", "⟦⇧ ❖ G⟧"),
            ("New folder", "⟦⇧ ❖ N⟧"),
            ("Show hidden files", "⟦⇧ ❖ .⟧"),
            ("Open or save, cancel", "⟦↩⟧ ⟦⎋⟧"),
        ]),
    );
    b += "</div>";
    b += r#"<div class="tiles">"#;
    b += &win(
        "pointer",
        "With the pointer",
        "",
        "<ul class=\"facts\"><li>Click a tab to focus and raise its window; drag it to move the window.</li><li>⟦⇧⟧-drag a tab to slide it along the window's top edge.</li><li>Drop a tab among another window's tabs to stack the two; drag a background tab away to take it out, or ⟦⌥⟧-drag the one showing.</li><li>Double-click a tab, or its zoom box, to fill; middle-click it to send the window to the back; right-press it for the marking menu.</li><li>A window's edges and corners resize, a little beyond the frame.</li><li>Right-click the empty desktop for the app's whole menu.</li></ul>",
    );
    b += &win(
        "other-apps",
        "Apps from elsewhere",
        "",
        "<p>For apps not built with Aizome's kit, the compositor turns ⟦❖⟧ shortcuts into the Control ones they expect: ⟦❖ C⟧ is Control-C, and Control-Shift-C in a terminal. In terminals, ⟦❖ W⟧ and ⟦❖ Q⟧ close the window and quit politely. Settei calls this “Command key for all apps”, and it is on unless you turn it off.</p><p>Every one of these keys is listed in Settei › Keyboard, where each can be changed, and kept in <code>~/.config/aizome/settings.toml</code> under <code>[keys.bindings]</code>.</p>",
    );
    b += "</div>";
    Ok(Page {
        path: "/keys/".into(),
        title: "Aizome's keys".into(),
        description: "Super is the command key. The keys the Aizome desktop keeps for itself: the palette, the Toolchest, windows, tabs, Desks and screenshots.".into(),
        menu: Menu::Keys,
        body: b,
    })
}

pub fn design(site: &Site) -> Result<Page> {
    let t = &site.theme;
    let mut b = head(
        "Design",
        "<p class=\"eyebrow\">The design</p><h1>Indigo, dipped again and again.</h1><p class=\"lead\">Night is Tokyo Night, Tatami's palette, and its indigos carry their old dye names. Day is a nod to IRIX. High Contrast is Night with stronger edges and text, for low vision. Every colour and size on this page, and on this whole site, is read from Aizome's own theme when the site is built, so the two can't drift apart.</p>",
        "",
    );

    // The mark, drawn on its module grid.
    let mut grid = String::new();
    for i in 1..4 {
        write!(
            grid,
            r#"<line x1="{i}" y1="0" x2="{i}" y2="4"/><line x1="0" y1="{i}" x2="4" y2="{i}"/>"#
        )?;
    }
    b += &format!(
        r#"<div class="step">{text}<figure class="window mark-figure"><div class="tab" aria-hidden="true">mark</div><div class="frame"><svg class="mark-grid" viewBox="-0.25 -0.25 4.5 4.5" role="img" aria-label="Aizome's mark on its grid of four modules by four: an amber tab two modules wide at the top left, and three stripes one module tall, pale, mid and deep indigo."><rect class="mk-amber" x="0" y="0" width="2" height="1"/><rect class="mk-pale" x="0" y="1" width="4" height="1"/><rect class="mk-mid" x="0" y="2" width="4" height="1"/><rect class="mk-deep" x="0" y="3" width="4" height="1"/><g class="grid-lines">{grid}<rect x="0" y="0" width="4" height="4"/></g></svg><figcaption>Four modules by four. The tab is two by one; each stripe is one module tall.</figcaption></div></figure></div>"#,
        text = win(
            "mark",
            "The mark",
            "",
            "<p class=\"headline\">A window's amber tab over three dips of indigo.</p><p>The mark is four modules square: the tab, two modules by one, at the top left in kohaku amber, over three stripes, one module each, pale (kamenozoki), mid (hanada) and deep (ai). Its amber is fixed: it doesn't follow your focus colour. The Toolchest's button carries it.</p><p>Beside the word, ⟪藍染⟫ stands vertically like a seal.</p>",
        ),
    );

    // The dyes.
    let mut dyes = String::new();
    for d in &t.dyes {
        write!(
            dyes,
            r#"<li><span class="dye sw-dye-{name}" aria-hidden="true"></span><span class="dye-name">{name}</span>{kanji}<code>{hex}</code></li>"#,
            name = d.name,
            kanji = site.ja(&d.kanji)?,
            hex = d.hex,
        )?;
    }
    let mut focus = String::new();
    for (name, night, day) in &t.focus {
        write!(
            focus,
            r#"<li><span class="dye sw-focus-{name}-night" aria-hidden="true"></span><span class="dye sw-focus-{name}-day" aria-hidden="true"></span><span class="dye-name">{name}</span><code>{night}</code> <code>{day}</code></li>"#
        )?;
    }
    b += r#"<div class="tiles">"#;
    b += &win(
        "dyes",
        "The dyes",
        "",
        &format!(
            r#"<p>The indigos from lightest to darkest, and the amber: the names they carry in the theme, and on the calendar, the weather and the rings of Kura.</p><ul class="dyes">{dyes}</ul>"#
        ),
    );
    b += &win(
        "focus",
        "Focus colours",
        "",
        &format!(
            r#"<p>The focused window's tab and rail, focus rings, and whatever is current. Kohaku amber is the default; Settei offers three more. Each has a Night and a Day value.</p><ul class="dyes">{focus}</ul>"#
        ),
    );
    b += "</div>";

    // The palette, with what each colour is for.
    let mut rows = String::new();
    let night = t.palette("night")?;
    for (name, _, _) in &night.colors {
        let doc = t
            .docs
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, d)| d.as_str())
            .unwrap_or("");
        write!(
            rows,
            r#"<tr><th scope="row"><code>{css}</code><span class="usage">{doc}</span></th>"#,
            css = name.replace('_', "-"),
            doc = esc(doc),
        )?;
        for p in &t.palettes {
            let hex = p
                .colors
                .iter()
                .find(|(n, _, _)| n == name)
                .map(|(_, _, c)| c.as_str())
                .unwrap_or("");
            write!(
                rows,
                r#"<td><span class="sw sw-{id}-{css}" aria-hidden="true"></span><code>{hex}</code></td>"#,
                id = p.id,
                css = name.replace('_', "-"),
                hex = esc(hex),
            )?;
        }
        rows += "</tr>";
    }
    let mut heads = String::new();
    for p in &t.palettes {
        write!(heads, r#"<th scope="col">{}</th>"#, p.name)?;
    }
    b += &win(
        "palette",
        "The palette",
        "",
        &format!(
            r#"<p>Every colour a surface of Aizome uses, in Night, Day and High Contrast, with what the theme says it's for. This site is dressed in the same colours: its menu bar is <code>bar</code>, its windows <code>surface</code>, their tabs <code>focus</code> and <code>tab-idle</code>.</p><div class="table-wrap"><table class="palette"><thead><tr><th scope="col">Colour</th>{heads}</tr></thead><tbody>{rows}</tbody></table></div>"#
        ),
    );

    // Contrast, computed.
    let mut crow = String::new();
    for &(fg, bg, min, what) in PAIRS {
        write!(
            crow,
            r#"<tr><th scope="row">{what}<span class="usage"><code>{}</code> on <code>{}</code>, at least {min}:1</span></th>"#,
            fg.replace('_', "-"),
            bg.replace('_', "-"),
        )?;
        for p in &t.palettes {
            let ratio = t.contrast(p.id, fg, bg)?;
            write!(crow, r#"<td><span class="ratio">{ratio:.1}:1</span></td>"#)?;
        }
        crow += "</tr>";
    }
    b += &win(
        "contrast",
        "Contrast",
        "",
        &format!(
            r#"<p>Aizome's theme holds its text to 7:1 and its secondary text, selections and focus to 4.5:1. These are the pairs this site sets, measured when it was built; the build stops if one falls short.</p><div class="table-wrap"><table class="palette contrast"><thead><tr><th scope="col">Text, on</th>{heads}</tr></thead><tbody>{crow}</tbody></table></div>"#
        ),
    );

    // Type.
    b += &win(
        "type",
        "Type",
        "",
        r#"<div class="specimens">
<div class="spec"><p class="spec-meta">Archivo · interface text</p><p class="spec-sample spec-archivo">Every window wears a tab only as wide as its title.</p></div>
<div class="spec"><p class="spec-meta">Archivo at 87.5% width, 650 · tabs and menu titles</p><p class="spec-sample spec-tab">architecture.md</p></div>
<div class="spec"><p class="spec-meta">IBM Plex Mono · shortcuts, code, meters, clocks</p><p class="spec-sample spec-mono">00:03:19 ⟦⌃ ❖ ↑⟧</p></div>
<div class="spec"><p class="spec-meta">Noto Serif CJK JP · kanji only, the seal and dye names</p><p class="spec-sample spec-kanji">⟪藍染⟫ ⟪甕覗⟫ ⟪縹⟫ ⟪藍⟫ ⟪紺⟫ ⟪褐⟫ ⟪琥珀⟫</p></div>
</div>
<p>Shortcuts print their modifiers as glyphs, in the order ⌃ ⌥ ⇧ ❖, from Noto Sans Symbols 2. Every clock and meter uses tabular digits, so nothing shifts as numbers change.</p>"#,
    );

    // Metrics.
    let mut m = String::new();
    for (name, value, doc) in &t.metrics {
        write!(
            m,
            r#"<div><dt><code>{name}</code></dt><dd><strong>{value}pt</strong> {doc}</dd></div>"#,
            name = esc(&name.to_lowercase().replace('_', "-")),
            value = esc(value),
            doc = esc(doc),
        )?;
    }
    b += &step(
        &win(
            "frames",
            "Frames, tabs and effects",
            "",
            &format!(
                r#"<p>Frames are square, with a 1pt bevel and a hard shadow three points right and down: a solid fill, never blurred, whole pixels at every integer scale. Everything sits on a 4pt grid, and no pointer target is smaller than 24 points.</p><dl class="plain">{m}</dl><p>Three effects tiers, and Aizome must look finished in all of them. <strong>Lean</strong>, the default: hard shadows and 80 ms of motion, no frost. <strong>Full</strong>: translucent, frosted menus, panels and Deskbar, and 120 ms of motion. <strong>Austere</strong>: no shadows, no motion, nothing see-through, the least to draw. Motion moves layers and never repaints; reduced motion means none.</p>"#
            ),
        ),
        &site.shot(
            "full-effects.png",
            "The Full effects tier in Night: the Toolchest torn off over the city's lights, Sumi's menu at the pointer with File open, and the Deskbar, their frosted faces letting the wallpaper tint through.",
            "The Full tier: frosted faces with a fine grain and no blur. From the walkthrough session of 18 October 2026, run without the system bus, so the systray has no network or battery.",
            Frame::Window,
        )?,
    );

    // Speed.
    b += &win(
        "fast",
        "Fast by construction",
        "",
        r#"<p class="headline">A 4 ms budget, from an event to its frame, for every part of the desktop.</p>
<div class="cols"><p>Integer scales first, 1×, 2× and 3×, with a Size control that changes layout, not pixels. Every surface declares what it covers opaquely, so nothing under it is drawn. Only what changed is redrawn: a clock tick repaints the clock, moving a window composites without drawing anything again, and an idle desktop draws no frames at all.</p><p>The compositor composites on the GPU with GLES, and falls back to software by itself when there's no usable GPU. The shell and apps draw on the processor in Sukumo's render thread, damage-tracked, so they look and perform the same either way.</p></div>
<div class="table-wrap"><table class="numbers"><caption>Milliseconds of processor time from an event to its frame: half of frames within the first figure, 95 in 100 within the second. Release builds, on an Intel Core Ultra 7 365 laptop, 18 October 2026.</caption><thead><tr><th scope="col">What</th><th scope="col">Median</th><th scope="col">95th</th></tr></thead><tbody>
<tr><th scope="row">The compositor moving a window (GPU)</th><td>0.46</td><td>0.61</td></tr>
<tr><th scope="row">The compositor moving a window (software)</th><td>0.45</td><td>0.69</td></tr>
<tr><th scope="row">Switching Desks (GPU)</th><td>0.45</td><td>0.75</td></tr>
<tr><th scope="row">The clock's tick in the menu bar and Deskbar</th><td>0.11</td><td>0.13</td></tr>
<tr><th scope="row">A key typed in the palette</th><td>0.74</td><td>1.80</td></tr>
<tr><th scope="row">A character typed in a 2,000-line editor</th><td>0.29</td><td>0.51</td></tr>
<tr><th scope="row">Scrolling a 10,000-row list a notch</th><td>0.15</td><td>0.20</td></tr>
<tr><th scope="row">Settei: flipping a switch</th><td>0.20</td><td>0.23</td></tr>
<tr><th scope="row">Hako: ↓ in a column of 10,000 files</th><td>2.28</td><td>2.44</td></tr>
<tr><th scope="row">Myaku: a new reading of 600 processes</th><td>2.61</td><td>2.99</td></tr>
<tr><th scope="row">The login screen: typing a password</th><td>0.06</td><td>0.07</td></tr>
</tbody></table></div>"#,
    );
    Ok(Page {
        path: "/design/".into(),
        title: "Aizome's design: indigo, amber and a 4 ms budget".into(),
        description: "Aizome's mark, its dyes and palettes in Night, Day and High Contrast with their contrast measured, its type and metrics, and the 4 ms frame budget.".into(),
        menu: Menu::Design,
        body: b,
    })
}

pub fn install(site: &Site) -> Result<Page> {
    let mut b = head(
        "Install",
        &format!(
            "<p class=\"eyebrow\">Install</p><h1>Get Aizome.</h1><p class=\"lead\">Aizome is the desktop Sekka is made for, selected by default in Sekka's installer among the desktops you can pick. It also installs on any NixOS system with flakes.</p><p class=\"actions\"><a class=\"button primary\" href=\"{SEKKA_DOWNLOAD}\">Get it with Sekka</a><a class=\"button\" href=\"{SEKKA}\">sekka.computer</a></p>"
        ),
        "",
    );
    b += &step(
        &win(
            "sekka",
            "With Sekka",
            "",
            &format!(
                "<p class=\"headline\">Download, start, choose your disk.</p><ol class=\"steps\"><li>Download Sekka's ISO from <a href=\"{SEKKA_DOWNLOAD}\">sekka.computer</a> and write it to a USB drive: Kakehashi, Sekka's own writer, lists the builds, checks the one you pick and reads every byte back, on Linux, macOS or Windows. Any tool that writes a raw disk image works too.</li><li>Start the computer from it. The live session is Aizome, with Genkan, the installer, open: it's an Aizome app too.</li><li>Choose your disk, account, desktop, applications and location. Aizome is already chosen; add Plasma, GNOME, Tatami or Xfce beside it if you like. Genkan prepares the system while you review it, then copies it, so the installation itself is short. It erases the whole disk you choose.</li></ol>"
            ),
        ),
        &site.shot(
            "genkan.png",
            "Genkan's welcome page: nine steps down the side, from Welcome to Install, three promises, and a note that the installer erases one whole disk.",
            &format!("Genkan, Sekka's installer, an Aizome app, in its debug build's welcome preview with fixed sample data, cropped to its window. {VM}"),
            Frame::Lead,
        )?,
    );
    b += r#"<div class="tiles">"#;
    b += &win(
        "installed",
        "On a Sekka you already have",
        "",
        "<p>Open Yukimi › Desktops, choose Aizome and make it the default, and Apply. Yukimi asks for your password, builds the new system once and offers a restart when the change needs one. The same choices, in a Sekka system's configuration:</p><pre class=\"code\"><code>calamares.desktops = [ \"aizome\" /* and any others */ ];\ncalamares.defaultDesktop = \"aizome\";</code></pre><p>Aizome's own login screen is a choice there too, or from a terminal: <code>sekka-update --choose calamares.loginScreen=aizome</code>.</p>",
    );
    b += &win(
        "nixos",
        "On any NixOS",
        "",
        &format!(
            "<p>Add Sekka's flake and its Aizome module to your system's flake:</p><pre class=\"code\"><code>{}</code></pre><p><code>programs.aizome.enable</code> brings everything Aizome stands on: the session, portals, NetworkManager, BlueZ, PipeWire, UPower, power profiles, UDisks2, AccountsService, CUPS, GeoClue, GNOME Keyring, polkit, the fonts, and fish for the terminal. Add <code>programs.aizome.greeter.enable = true;</code> for Aizome's own login screen, or keep the one you have: Aizome adds itself to its sessions.</p><p>Add Sekka's binary cache too, and Aizome's packages download rather than compile:</p><pre class=\"code\"><code>{}</code></pre>",
            esc(NIXOS_FLAKE),
            esc(NIXOS_CACHE)
        ),
    );
    b += "</div>";
    b += &win(
        "first",
        "Your first minutes",
        "",
        r#"<p class="headline">Pick Aizome on the login screen, and Annai welcomes you with a short tour.</p>
<table class="keytable"><tbody>
<tr><th scope="row">Find any command, window, file, app or setting</th><td>⟦❖ Space⟧</td></tr>
<tr><th scope="row">Launch from the Toolchest</th><td>⟦❖ ⌥ Space⟧</td></tr>
<tr><th scope="row">A terminal</th><td>⟦❖ ↩⟧</td></tr>
<tr><th scope="row">Every Desk at once</th><td>⟦⌃ ❖ ↑⟧</td></tr>
<tr><th scope="row">The whole menu at the pointer</th><td>⟦❖⟧-right-click</td></tr>
<tr><th scope="row">Lock</th><td>⟦⌃ ❖ Q⟧</td></tr>
</tbody></table>
<p>Make it yours in Settei: Appearance for Night, Day and the focus colour, Displays, Network and Keyboard. <a href="/keys/">Every key</a></p>"#,
    );
    Ok(Page {
        path: "/install/".into(),
        title: "Install Aizome, with Sekka or on any NixOS".into(),
        description: "Aizome is Sekka's default desktop: download Sekka and its installer sets it up, or add Sekka's Aizome module to any NixOS system with flakes.".into(),
        menu: Menu::Install,
        body: b,
    })
}

pub fn fonts(site: &Site) -> Result<Page> {
    let mut b = head(
        "Fonts",
        "<p class=\"eyebrow\">Fonts</p><h1>The fonts this site serves.</h1><p class=\"lead\">Aizome's own faces, served from this site, never from a font service, both under the SIL Open Font License 1.1.</p>",
        "",
    );
    b += r#"<div class="tiles">"#;
    b += &win(
        "archivo",
        "Archivo",
        "",
        &format!(
            r#"<p class="spec-sample spec-archivo">Aizome's interface face, by Omnibus-Type, with variable width and weight.</p><p>Served as WOFF2, compressed from the font file Aizome installs. <a href="{}">License</a></p>"#,
            site.asset("fonts/Archivo-OFL.txt")?
        ),
    );
    b += &win(
        "plex",
        "IBM Plex Mono",
        "",
        &format!(
            r#"<p class="spec-sample spec-mono">Shortcuts, code, meters and clocks, by IBM.</p><p>Regular and SemiBold, as IBM publishes them for the web. <a href="{}">License</a></p>"#,
            site.asset("fonts/IBMPlexMono-OFL.txt")?
        ),
    );
    b += "</div>";
    b += &win(
        "outlines",
        "Outlines",
        "",
        "<p>The kanji are outlines drawn from Noto Serif CJK JP, and the key glyphs from Noto Sans Symbols 2, Noto Sans Symbols and Noto Sans Math, all also under the SIL Open Font License. No CJK or symbol font is downloaded.</p>",
    );
    Ok(Page {
        path: "/fonts/".into(),
        title: "Fonts on aizome.computer".into(),
        description: "The fonts this site serves, and their licenses.".into(),
        menu: Menu::Home,
        body: b,
    })
}

pub fn not_found(_site: &Site) -> Result<Page> {
    Ok(Page {
        path: "/404.html".into(),
        title: "Not found: Aizome".into(),
        description: "This page isn't here.".into(),
        menu: Menu::Home,
        body: format!(
            r#"<section class="window focus page-head lost"><div class="tab" aria-hidden="true">404</div><div class="frame">{mark}<div><h1>Nothing on this Desk.</h1><p class="lead">This page doesn't exist, or it moved. <a href="/">Start from the top</a>, or see <a href="{SOURCE}">Aizome's source</a>.</p></div></div></section>"#,
            mark = mark("lost-mark", None),
        ),
    })
}
