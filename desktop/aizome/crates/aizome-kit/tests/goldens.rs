//! Golden images of the gallery: controls in their states, text and
//! fields, lists, a Settings-like pane, an editor and a menu, in Night and
//! Day, at scale 1 (and one at scale 2), compared with
//! `tests/snapshots/*.png`. `AIZOME_UPDATE_GOLDENS=1` rewrites them.
//!
//! They need Aizome's fonts (`AIZOME_FONT_DIR`); without them they're
//! skipped, as the system's fonts would draw something else.

#[allow(dead_code)]
#[path = "../examples/gallery/views.rs"]
mod views;

use aizome_kit::Appearance;
use aizome_kit::prelude::*;
use aizome_kit::sidestep_ui::{Modifiers, NamedKey, PointerButton};
use aizome_kit::testing::{Harness, assert_golden};
use aizome_kit::theme::ThemeChoice;
use views::{Gallery, Msg, Page};

fn snapshot(name: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots").join(name)
}

/// A gallery on `page`, `w` × `h`, in `theme`, motion off (so what moves
/// stands still).
fn gallery(page: Page, theme: ThemeChoice, size: Size) -> Option<(Harness<Gallery>, WindowKey)> {
    let a = Appearance { theme, reduced_motion: true, ..Appearance::default() };
    let mut h = Harness::with(Gallery::default(), a, false);
    if !h.theme().has_ui_font() {
        eprintln!("skipped: Archivo isn't installed (set AIZOME_FONT_DIR)");
        return None;
    }
    let w = h.window();
    h.resize(w, size);
    h.send(Msg::Page(page));
    Some((h, w))
}

#[test]
fn controls_in_night_and_day() {
    for (theme, name) in [(ThemeChoice::Night, "controls-night.png"), (ThemeChoice::Day, "controls-day.png")] {
        let Some((mut h, w)) = gallery(Page::Controls, theme, Size::new(860.0, 560.0)) else { return };
        assert_golden(&mut h, w, snapshot(name), 1.0);
    }
}

#[test]
fn controls_hovered_pressed_and_focused() {
    let Some((mut h, w)) = gallery(Page::Controls, ThemeChoice::Night, Size::new(860.0, 300.0)) else { return };
    h.focus_window(w, true);
    // Focus the switch that's off from the keyboard, press the primary
    // button and hold it, and hover the first.
    let off = h.find_id(w, "toggle-off").unwrap();
    for _ in 0..30 {
        h.named(w, NamedKey::Tab, Modifiers::NONE);
        if h.focused_rect(w) == Some(off) {
            break;
        }
    }
    assert_eq!(h.focused_rect(w), Some(off));
    let primary = h.find_id(w, "primary").unwrap();
    h.press(w, primary.center(), PointerButton::Left, 1);
    assert_golden(&mut h, w, snapshot("controls-states-night.png"), 1.0);
    h.release(w, primary.center(), PointerButton::Left);
}

#[test]
fn fields_and_text() {
    let Some((mut h, w)) = gallery(Page::Fields, ThemeChoice::Night, Size::new(860.0, 520.0)) else { return };
    h.focus_window(w, true);
    let name = h.find_id(w, "name").unwrap();
    h.click(w, name.center());
    h.command(w, 'a');
    assert_golden(&mut h, w, snapshot("fields-night.png"), 1.0);
}

#[test]
fn lists_tables_and_outlines() {
    let Some((mut h, w)) = gallery(Page::Lists, ThemeChoice::Night, Size::new(860.0, 300.0)) else { return };
    assert_golden(&mut h, w, snapshot("lists-night.png"), 1.0);
}

#[test]
fn a_settings_pane_in_day() {
    let Some((mut h, w)) = gallery(Page::Settings, ThemeChoice::Day, Size::new(860.0, 460.0)) else { return };
    assert_golden(&mut h, w, snapshot("settings-day.png"), 1.0);
}

#[test]
fn an_editor_at_scale_two() {
    let Some((mut h, w)) = gallery(Page::Editor, ThemeChoice::Night, Size::new(640.0, 300.0)) else { return };
    h.focus_window(w, true);
    let e = h.find_id(w, "editor").unwrap();
    h.click(w, Point::new(e.x0 + 300.0, e.y0 + 8.0 + 21.0 * 9.0 + 10.0));
    assert_golden(&mut h, w, snapshot("editor-night@2x.png"), 2.0);
}

#[test]
fn a_menu_from_the_in_window_bar() {
    let a = Appearance { theme: ThemeChoice::Night, reduced_motion: true, ..Appearance::default() };
    let mut h = Harness::with(Gallery::default(), a, true);
    if !h.theme().has_ui_font() {
        return;
    }
    let w = h.window();
    h.resize(w, Size::new(560.0, 300.0));
    h.focus_window(w, true);
    h.command(w, '1');
    assert_eq!(h.program().page, Page::Controls);
    // Open the standard app menu. Its height changes when optional roles
    // are omitted, so identify it by its title position, not its height.
    h.click(w, Point::new(12.0, 12.0));
    assert_eq!(h.popups(w).len(), 1, "the Gallery app menu opened");
    h.named(w, NamedKey::ArrowDown, Modifiers::NONE);
    h.named(w, NamedKey::ArrowDown, Modifiers::NONE);
    assert_golden(&mut h, w, snapshot("menu-night.png"), 1.0);
}
