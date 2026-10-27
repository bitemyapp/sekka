//! The boards' scene, for the shell's headless tests: Desks Mail, Build and
//! Write; Editor focused with its menus published; Terminal, Files,
//! Browser and Mail running.

#![allow(dead_code)]

use std::path::{Path, PathBuf};

use aizome_ipc::{DeskInfo, OutputInfo, Rect as IpcRect, WindowInfo};
use aizome_kit::prelude::*;
use aizome_kit::testing::Harness;
use aizome_kit::theme::{Appearance, ThemeChoice};
use aizome_shell::apps::server::ServerEvent;
use aizome_shell::clock::Local;
use aizome_shell::ipc::{IpcEvent, SnapshotBox};
use aizome_shell::menubar::{NetGlyph, Tray};
use aizome_shell::shell::{Msg, Options, Shell};
use aizome_shell::systray::meters::Reading;
use aizome_shell::wm::Snapshot;
use aizome_shell_proto as proto;

pub const W: f64 = 1440.0;
pub const H: f64 = 900.0;

pub fn output() -> aizome_kit::Output {
    aizome_kit::Output {
        id: 1,
        name: "eDP-1".into(),
        connector: Some("eDP-1".into()),
        description: None,
        make: "BOE".into(),
        model: "NE135".into(),
        frame: Rect::new(0.0, 0.0, W, H),
        scale: 2.0,
        integer_scale: 2,
        physical_size: None,
        refresh_hz: 60.0,
        work_area: None,
    }
}

fn win(id: u64, app: &str, title: &str, pid: i32) -> WindowInfo {
    WindowInfo {
        id,
        app_id: app.into(),
        title: title.into(),
        pid: Some(pid),
        desk: Some(3),
        output: Some("eDP-1".into()),
        ..Default::default()
    }
}

/// The boards' windows.
pub fn snapshot() -> Snapshot {
    let desks = vec![
        DeskInfo { index: 1, id: 1, name: "Mail".into(), windows: 1, current: false, folder: None, output: None },
        DeskInfo { index: 2, id: 2, name: "Build".into(), windows: 2, current: false, folder: None, output: None },
        DeskInfo {
            index: 3,
            id: 3,
            name: "Write".into(),
            windows: 4,
            current: true,
            folder: Some("~/work".into()),
            output: None,
        },
    ];
    let mut roadmap = win(2, "org.aizome.Sumi", "roadmap.md", 100);
    roadmap.minimised = true;
    let mut cargo = win(3, "foot", "cargo test", 200);
    cargo.group = Some(1);
    cargo.group_members = vec![3, 4];
    let mut side = win(4, "org.aizome.Hako", "sidestep", 300);
    side.group = Some(1);
    side.group_members = vec![3, 4];
    let mut downloads = win(5, "org.aizome.Hako", "Downloads", 300);
    downloads.desk = Some(1);
    let mut arch = win(1, "org.aizome.Sumi", "architecture.md", 100);
    arch.focused = true;
    let windows = vec![
        arch,
        roadmap,
        cargo,
        side,
        downloads,
        win(6, "firefox", "Aizome — design", 400),
        win(7, "firefox", "Sidestep issues", 400),
        win(8, "firefox", "docs.rs", 400),
        win(9, "thunderbird", "Inbox", 500),
    ];
    let outputs = vec![OutputInfo {
        name: "eDP-1".into(),
        enabled: true,
        scale: 2.0,
        logical: IpcRect::new(0, 0, W as i32, H as i32),
        usable: IpcRect::new(0, 24, W as i32, H as i32 - 24),
        ..Default::default()
    }];
    Snapshot { windows, desks, current_desk: 3, focused: Some(1), outputs }
}

/// Editor's menus, as the kit publishes them.
pub fn editor_menus() -> Vec<proto::Menu> {
    let item = |id: u32, label: &str| proto::Item { id, label: label.into(), enabled: true, ..Default::default() };
    let cmd = |key: &str| Some(proto::Shortcut { key: key.into(), command: true, ..Default::default() });
    let menu = |id: u32, title: &str, items: Vec<proto::Item>| proto::Menu { id, title: title.into(), items };
    vec![
        menu(
            1,
            "Editor",
            vec![
                item(10, "About Editor"),
                proto::Item { kind: proto::ItemKind::Separator, ..Default::default() },
                proto::Item { shortcut: cmd("H"), role: Some("hide".into()), ..item(11, "Hide Editor") },
                proto::Item { shortcut: cmd("Q"), role: Some("quit".into()), ..item(12, "Quit Editor") },
            ],
        ),
        menu(
            2,
            "File",
            vec![
                proto::Item { shortcut: cmd("N"), ..item(20, "New") },
                proto::Item { shortcut: cmd("O"), ..item(21, "Open…") },
            ],
        ),
        menu(
            3,
            "Edit",
            vec![
                proto::Item { shortcut: cmd("Z"), ..item(30, "Undo Bold") },
                proto::Item {
                    shortcut: Some(proto::Shortcut {
                        key: "Z".into(),
                        command: true,
                        shift: true,
                        ..Default::default()
                    }),
                    enabled: false,
                    ..item(31, "Redo")
                },
                proto::Item { shortcut: cmd("X"), ..item(32, "Cut") },
                proto::Item { shortcut: cmd("C"), ..item(33, "Copy") },
                proto::Item { shortcut: cmd("V"), ..item(34, "Paste") },
            ],
        ),
        menu(4, "Search", vec![item(40, "Find…")]),
        menu(
            5,
            "Format",
            vec![
                proto::Item { shortcut: cmd("B"), ..item(50, "Bold") },
                proto::Item { shortcut: cmd("I"), ..item(51, "Italic") },
                proto::Item { shortcut: cmd("E"), ..item(52, "Code") },
            ],
        ),
        menu(
            6,
            "Window",
            vec![proto::Item { shortcut: cmd("M"), role: Some("minimise".into()), ..item(60, "Minimise") }],
        ),
        menu(7, "Help", vec![item(70, "Editor Help")]),
    ]
}

pub fn tray() -> Tray {
    Tray {
        network: Some(NetGlyph::Wifi(4)),
        sound: Some((0.6, false)),
        battery: Some((84.0, false)),
        cpu: true,
        memory: true,
        ..Tray::default()
    }
}

pub fn meters() -> Reading {
    Reading { cores: vec![18.0, 71.0, 12.0, 25.0, 12.0, 20.0, 6.0, 12.0], memory: 38.0, total: 22.0 }
}

pub fn appearance(day: bool) -> Appearance {
    Appearance { theme: if day { ThemeChoice::Day } else { ThemeChoice::Night }, ..Appearance::default() }
}

/// The shell headless, showing the boards' scene.
pub fn scene(day: bool) -> Harness<Shell> {
    let mut h = Harness::with(Shell::new(Options::default()), appearance(day), false);
    h.set_outputs(vec![output()]);
    h.send(Msg::Ipc(IpcEvent::Connected(Box::new(SnapshotBox(snapshot())))));
    h.send(Msg::Server(ServerEvent::Message(
        1,
        proto::AppMessage::Hello { app_id: "org.aizome.Sumi".into(), pid: 100, name: "Editor".into() },
    )));
    h.send(Msg::Server(ServerEvent::Message(1, proto::AppMessage::Menus { menus: editor_menus() })));
    {
        let s = h.program_mut();
        s.tray = tray();
        s.meters = meters();
        s.now = Local { year: 2026, month: 10, day: 17, hour: 21, minute: 42, second: 0, weekday: 6 };
    }
    h.send(Msg::Nothing);
    // Layer surfaces are the compositor's size.
    for k in h.windows() {
        let size = match h.title(k).as_str() {
            "Menu bar" => Some(Size::new(W, 24.0)),
            "Desktop" => Some(Size::new(W, H)),
            _ => None,
        };
        if let Some(s) = size {
            h.resize(k, s);
        }
    }
    h
}

/// The window titled `title`.
pub fn window_titled(h: &Harness<Shell>, title: &str) -> WindowKey {
    h.windows().into_iter().find(|k| h.title(*k) == title).unwrap_or_else(|| panic!("no window {title}"))
}

/// Where screenshots of the tests go: `target/shell-shots/`.
pub fn shots() -> PathBuf {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/shell-shots");
    let dir = std::env::var_os("CARGO_TARGET_DIR").map(|d| PathBuf::from(d).join("shell-shots")).unwrap_or(dir);
    std::fs::create_dir_all(&dir).expect("the shots folder");
    dir
}

/// Where golden images live: `tests/golden/`.
pub fn golden(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden").join(name)
}

/// Three unread mails and the boards' Shelf.
pub fn with_mail_and_shelf(h: &mut Harness<Shell>) {
    use aizome_shell::notifications::server::NoteEvent;
    use aizome_shell::notifications::store::Incoming;
    for (i, (summary, body)) in [
        ("Build passed", "sidestep-ui: 61 tests ok"),
        ("Lunch?", "There's ramen downstairs"),
        ("Re: tiles at fractional scale", "“Half a pixel off rather than blurred…”"),
    ]
    .into_iter()
    .enumerate()
    {
        let n = Incoming {
            app_name: "Mail".into(),
            summary: summary.into(),
            body: body.into(),
            desktop_entry: Some("thunderbird".into()),
            actions: vec![("default".into(), "Open".into())],
            ..Incoming::default()
        };
        h.send(Msg::Note(NoteEvent::Notify(10 + i as u32, Box::new(n))));
    }
    let s = h.program_mut();
    s.shelf.items = vec![
        aizome_shell::shelf::ShelfItem::File { path: "/home/u/wallpaper-draft.png".into() },
        aizome_shell::shelf::ShelfItem::Text { text: "damage is per layer, not per window".into() },
        aizome_shell::shelf::ShelfItem::Folder { path: "/home/u/release-notes".into() },
    ];
    h.send(Msg::Nothing);
}

/// The boards' wallpapers.
pub fn wallpaper(day: bool) -> PathBuf {
    let name = if day { "tokyo-at-dusk.jpg" } else { "da-nang-at-night.jpg" };
    PathBuf::from("/home/callen/work/aizome-design/reference/wallpapers").join(name)
}

/// Windows composited as the compositor would place them: (window, its top
/// left in points), bottom first, at `scale`.
pub fn compose(h: &mut Harness<Shell>, layers: &[(WindowKey, Point)], size: Size, scale: f64) -> (u32, u32, Vec<u8>) {
    let (w, hh) = ((size.width * scale) as u32, (size.height * scale) as u32);
    let mut out = vec![0u8; (w * hh * 4) as usize];
    for (k, at) in layers {
        let (lw, lh, px) = h.render_rgba(*k, scale);
        let (ox, oy) = ((at.x * scale).round() as i64, (at.y * scale).round() as i64);
        for y in 0..lh as i64 {
            for x in 0..lw as i64 {
                let (dx, dy) = (ox + x, oy + y);
                if dx < 0 || dy < 0 || dx >= w as i64 || dy >= hh as i64 {
                    continue;
                }
                let s = &px[((y * lw as i64 + x) * 4) as usize..][..4];
                let d = &mut out[((dy * w as i64 + dx) * 4) as usize..][..4];
                let sa = f32::from(s[3]) / 255.0;
                let da = f32::from(d[3]) / 255.0;
                let oa = sa + da * (1.0 - sa);
                for i in 0..3 {
                    let c = (f32::from(s[i]) * sa + f32::from(d[i]) * da * (1.0 - sa)) / oa.max(1e-6);
                    d[i] = c.round().clamp(0.0, 255.0) as u8;
                }
                d[3] = (oa * 255.0).round() as u8;
            }
        }
    }
    (w, hh, out)
}
