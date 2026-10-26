//! Shashin, run headlessly: opening a picture among its folder's, zoom,
//! pan, pinch, turning and saving, the Trash and Undo, the wallpaper,
//! Show in Hako, copying, the filmstrip, animations, SVGs, Get Info, full
//! screen and the slideshow; and golden images of its views.

use std::path::{Path, PathBuf};
use std::time::Duration;

use aizome_kit::prelude::*;
use aizome_kit::sidestep_ui::{Modifiers, NamedKey};
use aizome_kit::testing::{Harness, assert_golden};
use aizome_kit::theme::ThemeChoice;
use aizome_kit::{Appearance, WindowState};
use aizome_shashin::app::{Body, Wallpaper};
use aizome_shashin::view::Zoom;
use aizome_shashin::{Msg, Shashin};

mod common;
use common::*;

fn names(r: &Rig) -> Vec<String> {
    r.h.program().folder.iter().map(|p| p.file_name().unwrap().to_string_lossy().into_owned()).collect()
}

fn current(r: &Rig) -> String {
    r.h.program().current_path().map(|p| p.file_name().unwrap().to_string_lossy().into_owned()).unwrap_or_default()
}

fn key(r: &mut Rig, k: NamedKey) {
    let w = r.h.window();
    r.h.named(w, k, Modifiers::NONE);
}

// Behaviour.

#[test]
fn opens_a_picture_among_its_folder_in_hakos_order() {
    let mut r = rig("order", ThemeChoice::Night, "IMG_2.jpg");
    assert_eq!(
        names(&r),
        [
            "app.ico",
            "beach.png",
            "dance.gif",
            "IMG_2.jpg",
            "IMG_10.jpg",
            "logo.svg",
            "old.bmp",
            "scan.tiff",
            "sticker.webp"
        ]
    );
    assert_eq!(current(&r), "IMG_2.jpg");
    let w = r.h.window();
    assert_eq!(r.h.title(w), "IMG_2.jpg");
    key(&mut r, NamedKey::ArrowRight);
    assert_eq!(current(&r), "IMG_10.jpg");
    key(&mut r, NamedKey::ArrowLeft);
    key(&mut r, NamedKey::ArrowLeft);
    assert_eq!(current(&r), "dance.gif");
    key(&mut r, NamedKey::End);
    assert_eq!(current(&r), "sticker.webp");
    // Round from the last to the first.
    key(&mut r, NamedKey::ArrowRight);
    assert_eq!(current(&r), "app.ico");
    // Every kind decodes.
    for _ in 0..9 {
        settle(&mut r, 1.0);
        let p = r.h.program().pic.as_ref().expect("a picture");
        assert!(p.info.error.is_none(), "{}: {:?}", current(&r), p.info.error);
        assert!(p.info.dims.0 > 0);
        key(&mut r, NamedKey::ArrowRight);
    }
}

#[test]
fn the_neighbours_are_read_ahead() {
    let mut r = rig("ahead", ThemeChoice::Night, "IMG_2.jpg");
    settle(&mut r, 1.0);
    let ahead: Vec<String> =
        r.h.program().ahead.iter().map(|p| p.path.file_name().unwrap().to_string_lossy().into_owned()).collect();
    assert!(ahead.contains(&"IMG_10.jpg".to_owned()) && ahead.contains(&"dance.gif".to_owned()), "{ahead:?}");
    key(&mut r, NamedKey::ArrowRight);
    // Shown at once, from what was read ahead.
    assert!(r.h.program().pic.as_ref().is_some_and(|p| p.decoded));
}

#[test]
fn fits_then_zooms_about_the_pointer_and_pans() {
    let mut r = rig("zoom", ThemeChoice::Night, "IMG_2.jpg");
    let w = r.h.window();
    // 1600 × 1066 in a 960 × 610 view: fitted.
    let fit = r.h.program().z();
    assert_eq!(r.h.program().zoom, Zoom::Fit);
    assert!((fit - 0.5722).abs() < 0.001, "{fit}");
    r.h.command(w, '0');
    assert_eq!(r.h.program().zoom, Zoom::Fixed(1.0));
    let c = r.h.program().center;
    assert!((c.0 - 800.0).abs() < 1.0 && (c.1 - 533.0).abs() < 1.0, "{c:?}");
    // Drag the picture left and up: the view moves right and down.
    r.h.drag(w, Point::new(500.0, 300.0), Point::new(400.0, 250.0), 4);
    let c2 = r.h.program().center;
    assert!(((c2.0 - c.0) - 100.0).abs() < 1e-9 && ((c2.1 - c.1) - 50.0).abs() < 1e-9, "{c:?} {c2:?}");
    // ❖-scroll zooms about the pointer, a quarter a notch.
    r.h.key(w, aizome_kit::sidestep_ui::Key::Character("=".into()), Modifiers::SUPER);
    assert_eq!(r.h.program().z(), 1.5);
    r.h.command(w, '-');
    assert_eq!(r.h.program().z(), 1.0);
    // A pinch.
    r.h.pinch(w, Point::new(300.0, 300.0), 2.0);
    assert!((r.h.program().z() - 2.0).abs() < 1e-9);
    // Double-click goes back to the fit.
    r.h.press(w, Point::new(300.0, 300.0), aizome_kit::sidestep_ui::PointerButton::Left, 2);
    r.h.release(w, Point::new(300.0, 300.0), aizome_kit::sidestep_ui::PointerButton::Left);
    assert_eq!(r.h.program().zoom, Zoom::Fit);
    r.h.command(w, '9');
    assert_eq!(r.h.program().zoom, Zoom::Fit);
}

#[test]
fn draws_a_picture_made_for_the_zoom_once_it_settles() {
    let mut r = rig("tiles", ThemeChoice::Night, "IMG_2.jpg");
    let w = r.h.window();
    for zoom in [None, Some(0.75), Some(1.0), Some(1.5), Some(3.0)] {
        match zoom {
            None => r.h.send(Msg::Fit),
            Some(z) => {
                r.h.send(Msg::ActualSize);
                if z != 1.0 {
                    r.h.pinch(w, Point::new(480.0, 300.0), z);
                }
            }
        }
        settle(&mut r, 2.0);
        let p = r.h.program();
        let z = p.z();
        let pic = p.pic.as_ref().unwrap();
        let exact = pic.tiles.iter().any(|t| aizome_shashin::app::same_zoom(t.z, z))
            || (z > 2.0 && pic.tiles.iter().any(|t| t.z == 1.0));
        assert!(exact, "no drawing for {z} among {:?}", pic.tiles.iter().map(|t| t.z).collect::<Vec<_>>());
        // At 2×, the scale the window draws at.
        assert_eq!(p.scale, 2.0);
    }
}

#[test]
fn turns_and_saves_without_reencoding_a_jpeg() {
    let mut r = rig("turn", ThemeChoice::Night, "IMG_2.jpg");
    let w = r.h.window();
    let path = r.dir.join("IMG_2.jpg");
    let before = std::fs::read(&path).unwrap();
    r.h.command(w, 'r');
    assert_eq!(r.h.program().shown_dims(), (1066.0, 1600.0));
    assert_eq!(r.h.title(w), "IMG_2.jpg — Turned");
    settle(&mut r, 1.0);
    // Turned drawings were made.
    assert!(r.h.program().pic.as_ref().unwrap().tiles.iter().any(|t| t.turns == 1));
    r.h.command(w, 's');
    settle(&mut r, 1.0);
    let after = std::fs::read(&path).unwrap();
    assert_eq!(aizome_shashin::orient::jpeg_orientation(&after), 6);
    // The compressed picture is the same bytes.
    assert!(after.ends_with(&before[before.len() - 4096..]));
    assert_eq!(r.h.title(w), "IMG_2.jpg");
    assert_eq!(r.h.program().shown_dims(), (1066.0, 1600.0), "upright by its tag now");
    assert!(r.h.program().note.as_ref().is_some_and(|n| n.text.contains("without re-encoding")));
    // A PNG is written again, turned.
    r.h.send(Msg::Go(1));
    settle(&mut r, 1.0);
    r.h.command(w, 'l');
    r.h.command(w, 's');
    settle(&mut r, 1.0);
    let png = image::open(r.dir.join("beach.png")).unwrap().to_rgba8();
    assert_eq!(png, image::imageops::rotate270(&sprite()));
}

#[test]
fn unsaved_turns_ask_before_quitting() {
    let mut r = rig("ask", ThemeChoice::Night, "beach.png");
    let w = r.h.window();
    r.h.command(w, 'r');
    r.h.command(w, 'q');
    assert!(r.h.texts(w).iter().any(|t| t == "Save a turned picture?"), "{:?}", r.h.texts(w));
    r.h.click_text(w, "Don't Save");
    assert!(r.h.platform.quit);
}

#[test]
fn moves_to_the_trash_and_back() {
    let mut r = rig("trash", ThemeChoice::Night, "IMG_2.jpg");
    let w = r.h.window();
    let path = r.dir.join("IMG_2.jpg");
    r.h.key(w, aizome_kit::sidestep_ui::Key::Named(NamedKey::Backspace), Modifiers::SUPER);
    assert!(!path.exists());
    assert!(r.dir.join(".trash-can/files/IMG_2.jpg").exists());
    assert_eq!(current(&r), "IMG_10.jpg");
    assert_eq!(r.h.program().folder.len(), 8);
    assert!(r.h.program().note.as_ref().is_some_and(|n| n.text.contains("Trash")));
    r.h.command(w, 'z');
    assert!(path.exists());
    assert_eq!(current(&r), "IMG_2.jpg");
    assert_eq!(r.h.program().folder.len(), 9);
    // And again.
    r.h.key(w, aizome_kit::sidestep_ui::Key::Character("z".into()), Modifiers::SUPER | Modifiers::SHIFT);
    assert!(!path.exists());
}

#[test]
fn sets_the_wallpaper_shows_in_hako_and_copies() {
    let mut r = rig("elsewhere", ThemeChoice::Night, "IMG_10.jpg");
    let w = r.h.window();
    r.h.send(Msg::SetWallpaper(Wallpaper::Night));
    settle(&mut r, 1.0);
    let s = aizome_config::Settings::load_from(&r.dir.join("settings.toml")).unwrap();
    assert_eq!(s.appearance.wallpaper_night.as_deref(), Some(r.dir.join("IMG_10.jpg").to_str().unwrap()));
    assert_eq!(s.appearance.wallpaper_day, None);
    r.h.send(Msg::ShowInHako);
    assert_eq!(
        r.h.program().commands,
        [vec!["aizome-hako".to_owned(), "--select".into(), r.dir.join("IMG_10.jpg").to_string_lossy().into_owned()]]
    );
    r.h.command(w, 'c');
    settle(&mut r, 1.0);
    assert!(r.h.program().copied.is_some_and(|n| n > 1000));
}

#[test]
fn wallpaper_uses_the_renderers_pixels_and_preserves_settings_on_failure() {
    use std::os::unix::fs::PermissionsExt;
    let mut r = rig("wallpaper-validation", ThemeChoice::Night, "beach.png");
    let settings = r.dir.join("settings.toml");
    // Every raster format the viewer advertises, all three destinations.
    // Misleading extensions are fine: the renderer reads the content.
    std::fs::copy(r.dir.join("beach.png"), r.dir.join("raster.svg")).unwrap();
    for file in ["beach.png", "IMG_2.jpg", "dance.gif", "sticker.webp", "old.bmp", "scan.tiff", "app.ico", "raster.svg"]
    {
        r.h.send(Msg::Open(vec![r.dir.join(file)]));
        settle(&mut r, 1.0);
        for which in [Wallpaper::Night, Wallpaper::Day, Wallpaper::Both] {
            let mut initial = aizome_config::Settings::default();
            initial.appearance.wallpaper_night = Some("previous-night.png".into());
            initial.appearance.wallpaper_day = Some("previous-day.png".into());
            initial.save_to(&settings).unwrap();
            r.h.send(Msg::SetWallpaper(which));
            settle(&mut r, 1.0);
            let got = aizome_config::Settings::load_from(&settings).unwrap();
            let path = r.dir.join(file).to_string_lossy().into_owned();
            assert_eq!(
                got.appearance.wallpaper_night.as_deref(),
                Some(if which == Wallpaper::Day { "previous-night.png" } else { &path })
            );
            assert_eq!(
                got.appearance.wallpaper_day.as_deref(),
                Some(if which == Wallpaper::Night { "previous-day.png" } else { &path })
            );
            assert!(r.h.program().note.as_ref().unwrap().text.starts_with("Set as"));
            assert!(aizome_face::wallpaper::prepare(&r.dir.join(file), 32, 24).is_some(), "{file}");
        }
    }
    // Fail after the viewer has already loaded a good picture: validation
    // must read the path again rather than trusting the cached preview.
    for failure in ["svg", "text", "truncated", "missing", "unreadable", "directory"] {
        let path = r.dir.join("changed.png");
        std::fs::copy(r.dir.join("beach.png"), &path).unwrap();
        r.h.send(Msg::Open(vec![path.clone()]));
        settle(&mut r, 1.0);
        match failure {
            "svg" => std::fs::write(&path, SVG).unwrap(),
            "text" => std::fs::write(&path, "not an image").unwrap(),
            "truncated" => {
                let bytes = std::fs::read(&path).unwrap();
                // Keep a valid PNG header while removing the pixel data.
                std::fs::write(&path, &bytes[..33]).unwrap();
            }
            "missing" => std::fs::remove_file(&path).unwrap(),
            "unreadable" => std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).unwrap(),
            "directory" => {
                std::fs::remove_file(&path).unwrap();
                std::fs::create_dir(&path).unwrap();
            }
            _ => unreachable!(),
        }
        let before = std::fs::read(&settings).unwrap();
        for which in [Wallpaper::Night, Wallpaper::Day, Wallpaper::Both] {
            r.h.send(Msg::SetWallpaper(which));
            settle(&mut r, 1.0);
            assert_eq!(std::fs::read(&settings).unwrap(), before, "{failure}, {which:?}");
            let note = &r.h.program().note.as_ref().unwrap().text;
            assert!(note.contains("Wallpaper unchanged"), "{failure}: {note}");
            assert!(aizome_face::wallpaper::prepare(&path, 32, 24).is_none(), "{failure}");
        }
        if failure == "directory" {
            std::fs::remove_dir(&path).unwrap();
        } else if path.exists() {
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
            std::fs::remove_file(&path).unwrap();
        }
    }
    // Recovery with the same live app and settings destination.
    r.h.send(Msg::Open(vec![r.dir.join("beach.png")]));
    settle(&mut r, 1.0);
    r.h.send(Msg::SetWallpaper(Wallpaper::Both));
    settle(&mut r, 1.0);
    let got = aizome_config::Settings::load_from(&settings).unwrap();
    assert_eq!(got.appearance.wallpaper_night, got.appearance.wallpaper_day);
    assert_eq!(got.appearance.wallpaper_night.as_deref(), r.dir.join("beach.png").to_str());
}

#[test]
fn the_filmstrip_shows_thumbnails_kept_in_the_freedesktop_cache() {
    let mut r = rig("strip", ThemeChoice::Night, "IMG_2.jpg");
    let w = r.h.window();
    r.h.key(w, aizome_kit::sidestep_ui::Key::Character("f".into()), Modifiers::NONE);
    assert!(r.h.program().filmstrip);
    settle(&mut r, 1.0);
    let ready = r.h.program().thumbs.values().filter(|t| matches!(t, aizome_shashin::app::Thumb::Ready(_))).count();
    assert_eq!(ready, 9);
    let cached = aizome_shashin::thumbs::thumb_path(&r.dir.join(".thumbnails"), &r.dir.join("IMG_2.jpg"));
    assert!(cached.exists());
    // Clicking a thumbnail goes to it: the slot right of the current one.
    r.h.click(w, Point::new(480.0 + 72.0, 640.0 - 38.0));
    assert_eq!(current(&r), "IMG_10.jpg");
}

#[test]
fn animations_play() {
    let mut r = rig("gif", ThemeChoice::Night, "dance.gif");
    let p = r.h.program().pic.as_ref().unwrap();
    assert!(matches!(p.info.body, Some(Body::Frames(..))));
    assert_eq!(p.frame, 0);
    r.h.advance(Duration::from_millis(210));
    assert_eq!(r.h.program().pic.as_ref().unwrap().frame, 1);
    r.h.advance(Duration::from_millis(400));
    assert_eq!(r.h.program().pic.as_ref().unwrap().frame, 0);
}

#[test]
fn svgs_are_drawn_at_the_zoom() {
    let mut r = rig("svg", ThemeChoice::Night, "logo.svg");
    let w = r.h.window();
    assert_eq!(r.h.program().pic.as_ref().unwrap().info.dims, (320, 240));
    for _ in 0..4 {
        r.h.key(w, aizome_kit::sidestep_ui::Key::Character("+".into()), Modifiers::SUPER | Modifiers::SHIFT);
    }
    assert_eq!(r.h.program().z(), 4.0);
    settle(&mut r, 1.0);
    let pic = r.h.program().pic.as_ref().unwrap();
    assert!(pic.tiles.iter().any(|t| t.z == 4.0), "{:?}", pic.tiles.iter().map(|t| t.z).collect::<Vec<_>>());
}

#[test]
fn get_info_tells_the_camera() {
    let mut r = rig("info", ThemeChoice::Night, "IMG_2.jpg");
    let w = r.h.window();
    r.h.command(w, 'i');
    let texts = r.h.texts(w);
    for want in ["JPEG picture", "1600 × 1066 pixels", "Fujifilm X100V", "camera"] {
        assert!(texts.iter().any(|t| t.to_lowercase().contains(&want.to_lowercase())), "{want} not in {texts:?}");
    }
    let settings = texts.iter().find(|t| t.contains("ISO")).expect("settings");
    assert!(
        settings.contains("23 mm")
            && settings.contains("ƒ/2")
            && settings.contains("1/250 s")
            && settings.contains("ISO 200"),
        "{settings}"
    );
    assert!(texts.iter().any(|t| t.contains("2026-10-17 06:12:00") || t.contains("2026:10:17")), "{texts:?}");
    r.h.named(w, NamedKey::Enter, Modifiers::NONE);
    assert!(r.h.program().info_sheet.is_none());
}

#[test]
fn what_isnt_a_picture_says_so() {
    let dir = folder("bad");
    std::fs::write(dir.join("broken.png"), b"\x89PNG\r\n\x1a\nnot really").unwrap();
    let mut app = Shashin::new();
    app.initial = vec![dir.join("broken.png")];
    app.dry_run = true;
    app.thumb_dir = dir.join(".thumbnails");
    let h = Harness::with(app, Appearance::default(), false);
    let mut r = Rig { h, dir };
    settle(&mut r, 1.0);
    let w = r.h.window();
    assert!(r.h.texts(w).iter().any(|t| t.contains("Shashin can't open “broken.png”")), "{:?}", r.h.texts(w));
}

#[test]
fn full_screen_and_the_slideshow() {
    let mut r = rig("show", ThemeChoice::Night, "IMG_2.jpg");
    let w = r.h.window();
    r.h.key(w, aizome_kit::sidestep_ui::Key::Character("F".into()), Modifiers::SUPER | Modifiers::SHIFT);
    assert_eq!(r.h.fullscreen_asked(w), Some(true));
    r.h.window_state(w, WindowState { fullscreen: true, maximized: false });
    assert!(r.h.program().fullscreen);
    // No toolbar in full screen.
    assert!(!r.h.texts(w).iter().any(|t| t.contains(" of 9")));
    r.h.advance(Duration::from_secs(5));
    assert_eq!(current(&r), "IMG_10.jpg");
    r.h.advance(Duration::from_secs(5));
    assert_eq!(current(&r), "logo.svg");
    key(&mut r, NamedKey::Escape);
    assert!(r.h.program().slideshow.is_none());
    assert_eq!(r.h.fullscreen_asked(w), Some(false));
    r.h.window_state(w, WindowState::default());
    assert!(!r.h.program().fullscreen);
    // The desktop's ⌃❖F, heard as the window's state.
    r.h.window_state(w, WindowState { fullscreen: true, maximized: false });
    assert!(r.h.program().fullscreen);
}

#[test]
fn a_second_launch_hands_its_files_over() {
    let dir = scratch("instance");
    let path = dir.join("img.png");
    sprite().save(&path).unwrap();
    // A socket of our own, not the session's.
    let name = format!("shashin-test-{}", std::process::id());
    let aizome_shashin::instance::Claim::First(listener) = aizome_shashin::instance::claim(&name, &[]) else {
        panic!("first")
    };
    let mut app = Shashin::new();
    app.instance = Some(listener);
    app.dry_run = true;
    app.thumb_dir = dir.join(".thumbnails");
    let h = Harness::with(app, Appearance::default(), false);
    let mut r = Rig { h, dir: dir.clone() };
    assert!(matches!(
        aizome_shashin::instance::claim(&name, std::slice::from_ref(&path)),
        aizome_shashin::instance::Claim::Handed
    ));
    for _ in 0..200 {
        r.h.send(Msg::Nothing);
        if r.h.program().current_path() == Some(&path) {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(r.h.program().current_path(), Some(&path));
    let _ = std::fs::remove_file(aizome_shashin::instance::socket_path(&name));
}

// Pictures.

fn golden(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden").join(format!("{name}.png"))
}

fn pictures(r: &mut Rig, name: &str) {
    let w = r.h.window();
    settle(r, 2.0);
    r.h.save_png(w, Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("shashin-{name}@2x.png")), 2.0);
    if !r.h.theme().has_ui_font() {
        eprintln!("skipping the golden {name}: Archivo isn't installed (set AIZOME_FONT_DIR=~/work/.aizome-fonts)");
        return;
    }
    settle(r, 1.0);
    assert_golden(&mut r.h, w, golden(name), 1.0);
}

#[test]
fn goldens() {
    for (theme, t) in [(ThemeChoice::Night, "night"), (ThemeChoice::Day, "day")] {
        let mut r = rig(&format!("golden-{t}"), theme, "IMG_2.jpg");
        pictures(&mut r, &format!("photo-{t}"));
        let w = r.h.window();
        r.h.key(w, aizome_kit::sidestep_ui::Key::Character("f".into()), Modifiers::NONE);
        pictures(&mut r, &format!("filmstrip-{t}"));
        r.h.command(w, 'i');
        pictures(&mut r, &format!("info-{t}"));
    }
    let mut r = rig("golden-pixels", ThemeChoice::Night, "beach.png");
    let w = r.h.window();
    r.h.send(Msg::ZoomPick(6));
    pictures(&mut r, "pixels-800-night");
    r.h.send(Msg::Go(5));
    pictures(&mut r, "svg-night");
    r.h.command(w, 'r');
    pictures(&mut r, "svg-turned-night");

    let mut app = Shashin::new();
    app.dry_run = true;
    let h = Harness::with(app, Appearance { theme: ThemeChoice::Day, ..Appearance::default() }, false);
    let mut r = Rig { h, dir: PathBuf::new() };
    pictures(&mut r, "empty-day");
}

#[test]
fn a_camera_jpeg_shows_its_thumbnail_while_it_decodes() {
    let d = scratch("preview");
    let bytes = camera_jpeg(3000, 2000);
    assert!(bytes.len() > 512 << 10, "{}", bytes.len());
    std::fs::write(d.join("DSCF0001.jpg"), bytes).unwrap();
    let r = rig_in(d, ThemeChoice::Night, "DSCF0001.jpg");
    assert_eq!(r.h.program().previews, 1);
    let p = r.h.program().pic.as_ref().unwrap();
    assert_eq!(p.info.dims, (3000, 2000));
    assert!(p.decoded);
}

#[test]
fn scrolling_pans_a_picture_larger_than_the_view() {
    let mut r = rig("scroll", ThemeChoice::Night, "IMG_2.jpg");
    let w = r.h.window();
    r.h.send(Msg::ActualSize);
    let c = r.h.program().center;
    r.h.scroll(w, Point::new(400.0, 300.0), Vec2::new(30.0, 40.0));
    let c2 = r.h.program().center;
    assert_eq!((c2.0 - c.0, c2.1 - c.1), (30.0, 40.0));
    // A fitted picture stays put.
    r.h.send(Msg::Fit);
    let c = r.h.program().center;
    r.h.scroll(w, Point::new(400.0, 300.0), Vec2::new(30.0, 40.0));
    assert_eq!(r.h.program().center, c);
}
