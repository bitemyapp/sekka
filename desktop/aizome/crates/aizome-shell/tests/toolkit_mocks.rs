//! Pictures of the Toolchest and the palette marking each app's toolkit,
//! in each style (none, tags, glyphs), over made-up apps whose toolkits
//! are given by hand: the mocks in `docs/images/toolkit-marks/`.

mod common;

use std::sync::Arc;

use aizome_kit::sidestep_ui::{Key, Modifiers, NamedKey};
use aizome_search::Scope;
use aizome_shell::actions::Action;
use aizome_shell::apps::toolkit::{MarkStyle, Toolkit};
use aizome_shell::menubar::{BarMsg, Slot};
use aizome_shell::shell::Msg;
use aizome_sys::apps::{AppIndex, Locale, XdgDirs};
use common::*;

const APPS: &[(&str, &str, &str, &str, Option<Toolkit>)] = &[
    // id, name, generic name, categories, toolkit
    ("org.aizome.Settei", "Settei", "Settings", "Settings;", None),
    ("org.aizome.Yukimi", "Yukimi", "Software", "System;PackageManager;", None),
    ("org.aizome.Hako", "Hako", "Files", "System;FileManager;", None),
    ("org.aizome.Myaku", "Myaku", "Activity", "System;Monitor;", None),
    ("org.aizome.Sumi", "Sumi", "Text Editor", "Utility;TextEditor;", None),
    ("org.aizome.Soroban", "Soroban", "Calculator", "Utility;Calculator;", None),
    ("com.mitchellh.ghostty", "Ghostty", "Terminal", "System;TerminalEmulator;", Some(Toolkit::Adwaita)),
    ("foot", "Foot", "Terminal", "System;TerminalEmulator;", Some(Toolkit::Own)),
    ("xterm", "XTerm", "Terminal", "System;TerminalEmulator;", Some(Toolkit::X11)),
    ("bottom", "bottom", "System Monitor", "System;Monitor;", Some(Toolkit::Terminal)),
    ("org.kde.partitionmanager", "KDE Partition Manager", "Partition Editor", "System;", Some(Toolkit::Qt6)),
    ("nvidia-settings", "NVIDIA X Server Settings", "", "Settings;", Some(Toolkit::Gtk3)),
    ("firefox", "Firefox", "Web Browser", "Network;WebBrowser;", Some(Toolkit::Gecko)),
    ("chromium-browser", "Chromium", "Web Browser", "Network;WebBrowser;", Some(Toolkit::Chromium)),
    ("signal", "Signal", "Messenger", "Network;InstantMessaging;", Some(Toolkit::Electron)),
    ("discord", "Discord", "Chat", "Network;InstantMessaging;", Some(Toolkit::Electron)),
    ("org.telegram.desktop", "Telegram", "Messenger", "Network;InstantMessaging;", Some(Toolkit::Qt6)),
    ("org.remmina.Remmina", "Remmina", "Remote Desktop", "Network;RemoteAccess;", Some(Toolkit::Gtk3)),
    ("spotify", "Spotify", "Music Player", "Audio;Music;Player;AudioVideo;", Some(Toolkit::Cef)),
    ("steam", "Steam", "Games", "Network;FileTransfer;Game;", Some(Toolkit::Cef)),
    ("dev.zed.Zed", "Zed", "Text Editor", "Utility;TextEditor;Development;IDE;", Some(Toolkit::Gpui)),
    ("code", "Visual Studio Code", "Text Editor", "Utility;TextEditor;Development;IDE;", Some(Toolkit::Electron)),
    ("slack", "Slack", "Team Chat", "Network;InstantMessaging;", Some(Toolkit::Electron)),
];

fn index() -> Arc<AppIndex> {
    let root = std::env::temp_dir().join(format!("aizome-toolkit-mocks-{}", std::process::id()));
    let dirs = XdgDirs::rooted(&root);
    let dir = dirs.data_home.join("applications");
    std::fs::create_dir_all(&dir).unwrap();
    for (id, name, generic, cats, _) in APPS {
        let g = if generic.is_empty() { String::new() } else { format!("GenericName={generic}\n") };
        std::fs::write(
            dir.join(format!("{id}.desktop")),
            format!("[Desktop Entry]\nType=Application\nName={name}\n{g}Exec=/bin/false\nCategories={cats}\n"),
        )
        .unwrap();
    }
    let index = AppIndex::load_with(dirs, Locale::default());
    std::fs::remove_dir_all(root).unwrap();
    Arc::new(index)
}

fn shell(day: bool, style: MarkStyle) -> aizome_kit::testing::Harness<aizome_shell::shell::Shell> {
    let mut h = scene(day);
    let s = h.program_mut();
    s.apps = Some(index());
    s.marks = style;
    for (id, _, _, _, tk) in APPS {
        if let Some(t) = tk {
            s.toolkits.insert((*id).to_owned(), *t);
        }
    }
    h.send(Msg::Nothing);
    h
}

fn key(h: &mut aizome_kit::testing::Harness<aizome_shell::shell::Shell>, popup: u32, k: NamedKey) {
    h.popup_keyboard(popup, Key::Named(k), Modifiers::default());
}

/// The bar with its open menus over the desktop, cropped to them.
fn menus_png(h: &mut aizome_kit::testing::Harness<aizome_shell::shell::Shell>, name: &str) {
    let scale = 2.0;
    let bar = window_titled(h, "Menu bar");
    let desk = window_titled(h, "Desktop");
    let (w, hh, mut px) = {
        let img = h.render(desk, scale);
        let (w, hh) = img.pixel_size();
        (w, hh, img.to_rgba8().unwrap().to_vec())
    };
    let mut layers = vec![(h.render(bar, scale), 0.0, 0.0)];
    let mut x1: f64 = 0.0;
    let mut y1: f64 = 0.0;
    for (p, r) in h.popups(bar) {
        layers.push((h.render_popup(p, scale), r.x0, r.y0));
        x1 = x1.max(r.x1);
        y1 = y1.max(r.y1);
    }
    for (img, ox, oy) in layers {
        let (pw, ph) = img.pixel_size();
        let src = img.to_rgba8().unwrap();
        let (ox, oy) = ((ox * scale).round() as i64, (oy * scale).round() as i64);
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
    }
    let (cw, ch) = ((((x1 + 24.0) * scale) as u32).min(w), (((y1 + 24.0) * scale) as u32).min(hh));
    let mut out = Vec::with_capacity((cw * ch * 4) as usize);
    for y in 0..ch {
        out.extend_from_slice(&px[((y * w) * 4) as usize..][..(cw * 4) as usize]);
    }
    aizome_kit::testing::write_png(&shots().join(name), cw, ch, &out).unwrap();
}

#[test]
fn mocks() {
    for day in [false, true] {
        for (style, sname) in [(MarkStyle::Off, "off"), (MarkStyle::Tag, "text"), (MarkStyle::Glyph, "icon")] {
            let theme = if day { "day" } else { "night" };
            for (step, cat) in [(5, "system"), (3, "internet")] {
                let mut h = shell(day, style);
                let bar = window_titled(&h, "Menu bar");
                h.send(Msg::Bar(bar, BarMsg::Press(Slot::Toolchest, aizome_kit::Rect::new(0.0, 0.0, 92.0, 24.0))));
                let (tc, _) = *h.popups(bar).last().unwrap();
                key(&mut h, tc, NamedKey::ArrowDown);
                key(&mut h, tc, NamedKey::ArrowRight);
                let (apps, _) = *h.popups(bar).last().unwrap();
                for _ in 0..step {
                    key(&mut h, apps, NamedKey::ArrowDown);
                }
                key(&mut h, apps, NamedKey::ArrowRight);
                menus_png(&mut h, &format!("mock-toolchest-{cat}-{sname}-{theme}.png"));
            }
            let mut h = shell(day, style);
            h.send(Msg::Run(Action::Palette { scope: Some(Scope::App), query: String::new() }));
            let p = window_titled(&h, "Palette");
            h.type_text(p, "s");
            h.send(Msg::Nothing);
            h.save_png(p, shots().join(format!("mock-palette-{sname}-{theme}.png")), 2.0);
        }
    }
}
