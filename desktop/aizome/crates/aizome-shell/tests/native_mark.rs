//! Aizome's mark after the names of Aizome's own apps, in the Toolchest's
//! Applications and the palette's apps, and nowhere else; and pictures of
//! it over Aizome's real desktop entries and made-up others, for choosing
//! the design, not checked:
//!
//! `NATIVE_MARK_DESIGNS=<folder> cargo test -p aizome-shell --test
//! native_mark -- --ignored`

mod common;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use aizome_kit::prelude::*;
use aizome_kit::sidestep_ui::{Key, Modifiers, NamedKey};
use aizome_search::Scope;
use aizome_shell::actions::Action;
use aizome_shell::apps::catalog;
use aizome_shell::menubar::{BarMsg, Slot};
use aizome_shell::shell::{Msg, Shell};
use aizome_sys::apps::{AppIndex, Locale, XdgDirs};
use common::*;

type H = aizome_kit::testing::Harness<Shell>;

/// Apps from elsewhere: id, name, generic name, categories.
const OTHERS: &[(&str, &str, &str, &str)] = &[
    ("com.mitchellh.ghostty", "Ghostty", "Terminal", "System;TerminalEmulator;"),
    ("foot", "Foot", "Terminal", "System;TerminalEmulator;"),
    ("xterm", "XTerm", "Terminal", "System;TerminalEmulator;"),
    ("bottom", "bottom", "System Monitor", "System;Monitor;"),
    ("org.kde.partitionmanager", "KDE Partition Manager", "Partition Editor", "System;"),
    ("io.github.Qalculate", "Qalculate!", "Calculator", "Utility;Calculator;"),
    ("org.xfce.mousepad", "Mousepad", "Text Editor", "Utility;TextEditor;"),
    ("firefox", "Firefox", "Web Browser", "Network;WebBrowser;"),
    ("signal", "Signal", "Messenger", "Network;InstantMessaging;"),
    ("slack", "Slack", "Team Chat", "Network;InstantMessaging;"),
    ("spotify", "Spotify", "Music Player", "Audio;Music;Player;AudioVideo;"),
    ("steam", "Steam", "Games", "Network;FileTransfer;Game;"),
    ("dev.zed.Zed", "Zed", "Text Editor", "Utility;TextEditor;Development;IDE;"),
];

/// An index of `OTHERS`, and of `aizome` (desktop entries, by file name).
fn index(aizome: &[(String, String)]) -> Arc<AppIndex> {
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!("aizome-native-mark-{}-{n}", std::process::id()));
    let dirs = XdgDirs::rooted(&root);
    let dir = dirs.data_home.join("applications");
    std::fs::create_dir_all(&dir).unwrap();
    for (id, name, generic, cats) in OTHERS {
        std::fs::write(
            dir.join(format!("{id}.desktop")),
            format!(
                "[Desktop Entry]\nType=Application\nName={name}\nGenericName={generic}\nExec=/bin/false\nCategories={cats}\n"
            ),
        )
        .unwrap();
    }
    for (file, text) in aizome {
        std::fs::write(dir.join(file), text).unwrap();
    }
    let index = AppIndex::load_with(dirs, Locale::default());
    std::fs::remove_dir_all(root).unwrap();
    Arc::new(index)
}

/// Two of Aizome's apps, written here.
fn some_aizome() -> Vec<(String, String)> {
    [("org.aizome.Hako", "Hako", "Files", "System;FileManager;"), ("org.aizome.Sumi", "Sumi", "Text Editor", "Utility;TextEditor;")]
        .into_iter()
        .map(|(id, name, generic, cats)| {
            (
                format!("{id}.desktop"),
                format!("[Desktop Entry]\nType=Application\nName={name}\nGenericName={generic}\nExec=/bin/false\nCategories={cats}\n"),
            )
        })
        .collect()
}

/// Aizome's own apps' desktop entries, from `apps/*/data`, as they install.
fn all_aizome() -> Vec<(String, String)> {
    let apps = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps");
    let mut out = Vec::new();
    for app in std::fs::read_dir(&apps).unwrap() {
        let Ok(data) = std::fs::read_dir(app.unwrap().path().join("data")) else { continue };
        for e in data {
            let p = e.unwrap().path();
            let name = p.file_name().unwrap().to_string_lossy().into_owned();
            if !name.ends_with(".desktop") {
                continue;
            }
            let text = std::fs::read_to_string(&p).unwrap();
            if !text.contains("NoDisplay=true") {
                out.push((name, text));
            }
        }
    }
    out
}

fn shell(day: bool, aizome: &[(String, String)]) -> H {
    let mut h = scene(day);
    h.program_mut().apps = Some(index(aizome));
    h.send(Msg::Nothing);
    h
}

/// The labels of the entries carrying the mark, at any depth, sorted.
fn marked(entries: &[aizome_shell::menus::model::Entry]) -> Vec<String> {
    fn walk(entries: &[aizome_shell::menus::model::Entry], out: &mut Vec<String>) {
        for e in entries {
            if e.mark {
                out.push(e.label.clone());
            }
            walk(&e.items, out);
        }
    }
    let mut out = Vec::new();
    walk(entries, &mut out);
    out.sort();
    out
}

#[test]
fn only_aizome_apps_carry_the_mark() {
    let h = shell(false, &some_aizome());
    // In the Toolchest: Applications' rows for Hako and Sumi, and no other
    // row (not System's Settei, which is Aizome's too).
    let menu = h.program().toolchest_menu();
    let apps = menu.items.iter().find(|e| e.label == "Applications").unwrap();
    assert_eq!(marked(&apps.items), ["Hako", "Sumi"]);
    assert_eq!(marked(&menu.items), ["Hako", "Sumi"]);
    // In the palette: the same two apps, and nothing that isn't an app.
    let items = h.program().palette_items();
    let mut titles: Vec<&str> = items.iter().filter(|i| i.mark).map(|i| i.title.as_str()).collect();
    titles.sort();
    assert_eq!(titles, ["Hako", "Sumi"]);
}

fn key(h: &mut H, popup: u32, k: NamedKey) {
    h.popup_keyboard(popup, Key::Named(k), Modifiers::default());
}

fn designs() -> PathBuf {
    let dir = PathBuf::from(std::env::var_os("NATIVE_MARK_DESIGNS").unwrap_or_else(|| shots().into_os_string()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// The desktop, the bar and its open menus, cropped to the menus.
fn save_menus(h: &mut H, name: &str) {
    let scale = 2.0;
    let bar = window_titled(h, "Menu bar");
    let desk = window_titled(h, "Desktop");
    let (w, hh, mut px) = compose(h, &[(desk, Point::ZERO), (bar, Point::ZERO)], Size::new(W, H), scale);
    let (mut x1, mut y1): (f64, f64) = (0.0, 0.0);
    for (p, r) in h.popups(bar) {
        let img = h.render_popup(p, scale);
        let (pw, ph) = img.pixel_size();
        let src = img.to_rgba8().unwrap();
        let (ox, oy) = ((r.x0 * scale).round() as i64, (r.y0 * scale).round() as i64);
        for y in 0..ph as i64 {
            for x in 0..pw as i64 {
                let (dx, dy) = (ox + x, oy + y);
                if dx < 0 || dy < 0 || dx >= w as i64 || dy >= hh as i64 {
                    continue;
                }
                let s = &src[((y * pw as i64 + x) * 4) as usize..][..4];
                let d = &mut px[((dy * w as i64 + dx) * 4) as usize..][..4];
                let a = f32::from(s[3]) / 255.0;
                for i in 0..3 {
                    d[i] = (f32::from(s[i]) * a + f32::from(d[i]) * (1.0 - a)).round() as u8;
                }
                d[3] = 255;
            }
        }
        x1 = x1.max(r.x1);
        y1 = y1.max(r.y1);
    }
    let (cw, ch) = ((((x1 + 24.0) * scale) as u32).min(w), (((y1 + 24.0) * scale) as u32).min(hh));
    let mut out = Vec::with_capacity((cw * ch * 4) as usize);
    for y in 0..ch {
        out.extend_from_slice(&px[((y * w) * 4) as usize..][..(cw * 4) as usize]);
    }
    aizome_kit::testing::write_png(&designs().join(name), cw, ch, &out).unwrap();
}

#[test]
#[ignore]
fn pictures_of_the_native_mark() {
    let aizome = all_aizome();
    for day in [false, true] {
        let t = if day { "day" } else { "night" };
        for cat in ["System", "Accessories"] {
            let mut h = shell(day, &aizome);
            let apps: Vec<_> = h.program().apps.as_deref().unwrap().apps();
            let step = catalog::by_category(&apps).iter().position(|(c, _)| *c == cat).unwrap();
            let bar = window_titled(&h, "Menu bar");
            h.send(Msg::Bar(bar, BarMsg::Press(Slot::Toolchest, Rect::new(0.0, 0.0, 92.0, 24.0))));
            let (tc, _) = *h.popups(bar).last().unwrap();
            key(&mut h, tc, NamedKey::ArrowDown);
            key(&mut h, tc, NamedKey::ArrowRight);
            let (list, _) = *h.popups(bar).last().unwrap();
            for _ in 0..step {
                key(&mut h, list, NamedKey::ArrowDown);
            }
            key(&mut h, list, NamedKey::ArrowRight);
            save_menus(&mut h, &format!("toolchest-{}-{t}.png", cat.to_lowercase()));
        }
        let mut h = shell(day, &aizome);
        h.send(Msg::Run(Action::Palette { scope: Some(Scope::App), query: String::new() }));
        let p = window_titled(&h, "Palette");
        h.type_text(p, "s");
        h.send(Msg::Nothing);
        h.save_png(p, designs().join(format!("palette-apps-{t}.png")), 2.0);
    }
}
