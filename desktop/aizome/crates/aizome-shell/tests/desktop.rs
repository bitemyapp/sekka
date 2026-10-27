//! The desktop, headless: its icons, selection, and the whole screen as
//! the boards show it (wallpaper, icons, bar and Deskbar).

mod common;

use aizome_kit::prelude::*;
use aizome_shell::desktop::DesktopMsg;
use aizome_shell::menubar::{BarMsg, Slot};
use aizome_shell::shell::Msg;
use aizome_shell::widgets::drawn::DragPhase;
use common::*;

fn board_desktop(day: bool) -> aizome_kit::testing::Harness<aizome_shell::shell::Shell> {
    let mut h = scene(day);
    with_mail_and_shelf(&mut h);
    {
        let s = h.program_mut();
        s.user = ("callen".into(), "Chris Allen".into());
        s.volumes_fixture = vec![
            aizome_sys::disks::Volume {
                block: "/org/freedesktop/UDisks2/block_devices/nvme0n1p3".into(),
                name: "nvme0".into(),
                ..Default::default()
            },
            aizome_sys::disks::Volume {
                block: "/org/freedesktop/UDisks2/block_devices/loop0".into(),
                name: "loop0".into(),
                image_file: Some("/home/callen/sekka-nightly.iso".into()),
                ..Default::default()
            },
        ];
        if day {
            s.settings.appearance.wallpaper_day = Some(wallpaper(true).to_string_lossy().into_owned());
        } else {
            s.settings.appearance.wallpaper_night = Some(wallpaper(false).to_string_lossy().into_owned());
        }
        s.theme_day = day;
        s.load_wallpapers();
        // The notifications' line has had its moment.
        s.banner = None;
        s.banner_queue.clear();
    }
    h.send(Msg::Nothing);
    h
}

#[test]
fn icons_line_up_and_select() {
    let mut h = board_desktop(false);
    let d = window_titled(&h, "Desktop");
    let texts = h.texts(d);
    for want in ["callen", "nvme0", "sekka-nightly.iso", "Trash"] {
        assert!(texts.iter().any(|t| t == want), "{want} in {texts:?}");
    }
    h.send(Msg::Desktop(1, DesktopMsg::Select("trash".into(), false)));
    assert_eq!(h.program().desktop.selected, ["trash"]);
    h.send(Msg::Desktop(1, DesktopMsg::Band(DragPhase::Start(Point::new(0.0, 30.0)))));
    h.send(Msg::Desktop(1, DesktopMsg::Band(DragPhase::Move(Point::new(120.0, 200.0)))));
    assert_eq!(h.program().desktop.selected, ["home", "vol:/org/freedesktop/UDisks2/block_devices/nvme0n1p3"]);
    h.send(Msg::Desktop(1, DesktopMsg::Band(DragPhase::End(Point::new(120.0, 200.0)))));
    assert!(h.program().desktop.band.is_none());
}

#[test]
fn the_systems_disk_opens_and_never_unmounts() {
    use aizome_shell::actions::{Action, DiskVerb};
    let mut h = board_desktop(false);
    {
        let s = h.program_mut();
        s.volumes_fixture[0].system = true;
        s.volumes_fixture[0].mount_point = Some("/".into());
        s.volumes_fixture[0].drive = Some("/org/freedesktop/UDisks2/drives/nvme".into());
        // Even if UDisks called its drive ejectable.
        s.volumes_fixture[0].ejectable = true;
    }
    h.send(Msg::Nothing);
    let icons = h.program().desktop_icons();
    let disk = icons.iter().find(|i| i.label == "nvme0").expect("the system's disk");
    assert_eq!(disk.glyph, aizome_kit::Icon::HardDrive);
    let menu = h.program().icon_menu(disk);
    let labels: Vec<&str> = menu.iter().map(|e| e.label.as_str()).filter(|l| !l.is_empty()).collect();
    assert_eq!(labels, ["Open", "Show in Disks", "Disk Usage"]);
    assert_eq!(menu[0].action, Action::Open { path: "/".into() });
    assert!(menu.iter().all(|e| !matches!(e.action, Action::Disk { verb: DiskVerb::Unmount | DiskVerb::Eject, .. })));
    // The image beside it: the disc, its file's name.
    let iso = icons.iter().find(|i| i.label == "sekka-nightly.iso").expect("the image");
    assert_eq!(iso.glyph, aizome_kit::Icon::Disc);
    // `[desktop] show_disks = false` hides both.
    let mut d = h.program().settings.desktop_settings();
    d.show_disks = false;
    h.program_mut().settings.set_shell_section("desktop", &d);
    h.send(Msg::SettingsChanged);
    assert!(h.program().desktop_icons().iter().all(|i| i.label != "nvme0" && i.label != "sekka-nightly.iso"));
}

#[test]
fn icons_move_where_theyre_dropped() {
    let mut h = board_desktop(false);
    h.send(Msg::Desktop(1, DesktopMsg::IconDrag("home".into(), DragPhase::Start(Point::new(40.0, 60.0)))));
    h.send(Msg::Desktop(1, DesktopMsg::IconDrag("home".into(), DragPhase::Move(Point::new(400.0, 300.0)))));
    h.send(Msg::Desktop(1, DesktopMsg::IconDrag("home".into(), DragPhase::End(Point::new(400.0, 300.0)))));
    let p = h.program().desktop.positions.get("home").copied().unwrap();
    assert_eq!(p, Point::new(376.0, 284.0));
}

#[test]
fn the_whole_screen() {
    for day in [false, true] {
        let mut h = board_desktop(day);
        let desk = window_titled(&h, "Desktop");
        let bar = window_titled(&h, "Menu bar");
        let db = window_titled(&h, "Deskbar");
        let size = aizome_shell::deskbar::size(&h.program().deskbar_model());
        h.resize(db, size);
        let at = Point::new(W - size.width, 24.0);
        let (w, hh, px) = compose(&mut h, &[(desk, Point::ZERO), (bar, Point::ZERO), (db, at)], Size::new(W, H), 2.0);
        let name = if day { "desktop-day.png" } else { "desktop-night.png" };
        aizome_kit::testing::write_png(&shots().join(name), w, hh, &px).unwrap();
        let only = if day { "desktop-icons-day.png" } else { "desktop-icons-night.png" };
        aizome_kit::testing::assert_golden(&mut h, desk, golden(only), 1.0);
    }
}

/// Lay a popup's picture (premultiplied) over `px` at `at` (points, 2×).
fn over(px: &mut [u8], width: u32, img: &aizome_kit::Image, at: Point) {
    let (w, hh) = img.pixel_size();
    let src = img.to_premultiplied_rgba8().expect("pixels");
    let (ox, oy) = ((at.x * 2.0) as u32, (at.y * 2.0) as u32);
    for y in 0..hh {
        for x in 0..w {
            let (dx, dy) = (ox + x, oy + y);
            if dx >= width || (dy * width + dx) as usize * 4 >= px.len() {
                continue;
            }
            let s = ((y * w + x) * 4) as usize;
            let d = ((dy * width + dx) * 4) as usize;
            let a = src[s + 3] as u32;
            for c in 0..3 {
                px[d + c] = (src[s + c] as u32 + px[d + c] as u32 * (255 - a) / 255).min(255) as u8;
            }
        }
    }
}

#[test]
fn full_effects_frost_what_floats_over_the_wallpaper() {
    use aizome_kit::sidestep_ui::{Modifiers, NamedKey};
    for day in [false, true] {
        let mut h = board_desktop(day);
        let mut look = appearance(day);
        look.effects = aizome_kit::theme::Effects::Full;
        h.set_appearance(look);
        h.program_mut().panels_fixture = None;
        let desk = window_titled(&h, "Desktop");
        let bar = window_titled(&h, "Menu bar");
        let db = window_titled(&h, "Deskbar");
        let size = aizome_shell::deskbar::size(&h.program().deskbar_model());
        h.resize(db, size);
        h.send(Msg::Run(aizome_shell::actions::Action::Palette { scope: None, query: String::new() }));
        let pal = window_titled(&h, "Palette");
        let ps = h.size(pal);
        let pal_at = Point::new(((W - ps.width) / 2.0).round(), 160.0);
        let (w, hh, mut px) = compose(
            &mut h,
            &[(desk, Point::ZERO), (bar, Point::ZERO), (db, Point::new(W - size.width, 24.0)), (pal, pal_at)],
            Size::new(W, H),
            2.0,
        );
        // The Toolchest, open over the desktop's left.
        h.send(Msg::Palette(aizome_shell::palette::PaletteEvent::Ui(aizome_shell::palette::view::PaletteMsg::Close)));
        h.send(Msg::Bar(bar, BarMsg::Press(Slot::Toolchest, Rect::new(0.0, 0.0, 92.0, 24.0))));
        let (menu, at) = h.popups(bar)[0];
        h.named(bar, NamedKey::ArrowDown, Modifiers::NONE);
        let img = h.render_popup(menu, 2.0);
        over(&mut px, w, &img, at.origin());
        let name = if day { "frosted-day.png" } else { "frosted-night.png" };
        aizome_kit::testing::write_png(&shots().join(name), w, hh, &px).unwrap();
    }
}
