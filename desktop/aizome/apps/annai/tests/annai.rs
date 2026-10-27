//! Annai, run headlessly: stepping through the pages, "Show at login" and
//! the first run, Settei's doors, the scenes playing without
//! repainting more than themselves; and golden images of every page in
//! Night and Day.

use std::path::{Path, PathBuf};
use std::time::Duration;

use aizome_annai::app::{PAGES, SCENE_HEIGHT};
use aizome_annai::{Annai, Msg, state};
use aizome_kit::Appearance;
use aizome_kit::layout::Point;
use aizome_kit::sidestep_ui::{Modifiers, NamedKey};
use aizome_kit::testing::{Harness, assert_golden};
use aizome_kit::theme::ThemeChoice;

fn scratch(name: &str) -> PathBuf {
    let d = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("annai-{name}"));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn annai(name: &str, theme: ThemeChoice, motion: bool) -> (Harness<Annai>, PathBuf) {
    let dir = scratch(name);
    let path = dir.join("state.json");
    let mut app = Annai::new(Some(path.clone()));
    app.dry_run = true;
    let h = Harness::with(app, Appearance { theme, reduced_motion: !motion, ..Appearance::default() }, false);
    (h, path)
}

#[test]
fn steps_through_the_pages_by_keys_buttons_and_dots() {
    let (mut h, _) = annai("steps", ThemeChoice::Night, false);
    let w = h.window();
    assert_eq!(h.title(w), "Welcome to Aizome");
    assert!(h.texts(w).iter().any(|t| t == "Welcome to Aizome"));
    h.named(w, NamedKey::ArrowRight, Modifiers::NONE);
    assert_eq!(h.program().page, 1);
    assert!(h.texts(w).iter().any(|t| t == "Stack windows by their tabs"));
    h.named(w, NamedKey::Enter, Modifiers::NONE);
    assert_eq!(h.program().page, 2);
    h.named(w, NamedKey::ArrowLeft, Modifiers::NONE);
    assert_eq!(h.program().page, 1);
    h.click_text(w, "Next");
    assert_eq!(h.program().page, 2);
    h.click_text(w, "Back");
    assert_eq!(h.program().page, 1);
    // ❖4 goes to the fourth page; the dots go anywhere.
    h.command(w, '4');
    assert_eq!(h.program().page, 3);
    h.named(w, NamedKey::End, Modifiers::NONE);
    assert_eq!(h.program().page, PAGES - 1);
    assert!(h.texts(w).iter().any(|t| t == "Done"));
    // Help ▸ Aizome Tour starts again.
    h.send(Msg::Go(0));
    assert_eq!(h.program().page, 0);
    // Done (or Return on the last page) closes it.
    h.named(w, NamedKey::End, Modifiers::NONE);
    h.named(w, NamedKey::Enter, Modifiers::NONE);
    assert!(h.windows().is_empty());
}

#[test]
fn the_first_run_is_recorded_and_show_at_login_kept() {
    let (mut h, path) = annai("state", ThemeChoice::Night, false);
    let w = h.window();
    let s = state::load(&path);
    assert!(s.shown.is_some(), "shown once it opens");
    assert!(!s.due(), "so --first-run won't show it again");
    h.click_text(w, "Show at login");
    assert!(state::load(&path).show_at_login);
    assert!(state::load(&path).due());
    h.click_text(w, "Show at login");
    assert!(!state::load(&path).due());
    // Escape closes it.
    h.named(w, NamedKey::Escape, Modifiers::NONE);
    assert!(h.windows().is_empty());
}

#[test]
fn the_last_page_opens_settei() {
    let (mut h, _) = annai("doors", ThemeChoice::Night, false);
    let w = h.window();
    h.send(Msg::Go(PAGES - 1));
    for pane in ["appearance", "displays", "network", "keyboard"] {
        let r = h.find_id(w, &format!("door-{pane}")).expect("a door");
        h.click(w, r.center());
    }
    let panes: Vec<String> = h.program().commands.iter().map(|c| c[2].clone()).collect();
    assert_eq!(panes, ["appearance", "displays", "network", "keyboard"]);
    assert!(h.program().commands.iter().all(|c| c[0] == "aizome-settei" && c[1] == "--pane"));
}

#[test]
fn scenes_play_and_repaint_only_themselves() {
    let (mut h, _) = annai("motion", ThemeChoice::Night, true);
    let w = h.window();
    let _ = h.render(w, 2.0);
    let t0 = h.program().t;
    for page in 0..PAGES {
        h.send(Msg::Go(page));
        let _ = h.render(w, 2.0);
        h.take_damage(w);
        h.advance(Duration::from_millis(330));
        assert!(h.program().t > t0 || page > 0);
        let damage = h.take_damage(w);
        assert!(!damage.is_empty(), "page {page} played");
        for d in damage {
            // (The focused scene's rectangle reaches 3pt further, for its ring.)
            assert!(d.y1 <= SCENE_HEIGHT + 3.5, "page {page}: {d:?} is outside the scene");
        }
    }
    // Under reduced motion it holds still.
    let (mut h, _) = annai("still", ThemeChoice::Night, false);
    let w = h.window();
    let _ = h.render(w, 1.0);
    h.take_damage(w);
    h.advance(Duration::from_secs(2));
    assert!(h.take_damage(w).is_empty());
    let _ = Point::ZERO;
}

fn golden(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden").join(format!("{name}.png"))
}

#[test]
fn goldens() {
    for (theme, t) in [(ThemeChoice::Night, "night"), (ThemeChoice::Day, "day")] {
        let (mut h, _) = annai(&format!("golden-{t}"), theme, false);
        let w = h.window();
        for page in 0..PAGES {
            h.send(Msg::Go(page));
            let name = format!("page{}-{t}", page + 1);
            h.save_png(w, Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("annai-{name}@2x.png")), 2.0);
            if !h.theme().has_ui_font() {
                eprintln!(
                    "skipping the golden {name}: Archivo isn't installed (set AIZOME_FONT_DIR=~/work/.aizome-fonts)"
                );
                continue;
            }
            assert_golden(&mut h, w, golden(&name), 1.0);
        }
    }
}
