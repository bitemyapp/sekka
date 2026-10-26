//! The palette, headless: what it finds for a query, its keys, and its
//! pictures.

mod common;

use aizome_kit::sidestep_ui::{Modifiers, NamedKey};
use aizome_search::Scope;
use aizome_shell::actions::Action;
use aizome_shell::palette::PaletteEvent;
use aizome_shell::palette::view::PaletteMsg;
use aizome_shell::shell::Msg;
use common::*;

fn open(h: &mut aizome_kit::testing::Harness<aizome_shell::shell::Shell>, query: &str) -> aizome_kit::WindowKey {
    h.send(Msg::Run(Action::Palette { scope: None, query: String::new() }));
    let p = window_titled(h, "Palette");
    if !query.is_empty() {
        h.type_text(p, query);
    }
    p
}

#[test]
fn typing_finds_the_focused_apps_commands_first() {
    let mut h = scene(false);
    let p = open(&mut h, "bold");
    let s = h.program().palette.as_ref().expect("open");
    assert_eq!(s.query, "bold");
    assert_eq!(s.sections[0].scope, Scope::Command);
    assert_eq!(s.sections[0].rows[0].item.title, "Bold");
    assert_eq!(s.sections[0].rows[0].item.trail, "Format");
    assert!(h.texts(p).iter().any(|t| t == "Bold"));
}

#[test]
fn windows_and_arithmetic() {
    let mut h = scene(false);
    open(&mut h, "cargo");
    let s = h.program().palette.as_ref().unwrap();
    assert!(s.sections.iter().any(|s| s.scope == Scope::Window && s.rows[0].item.title == "cargo test"));
    h.send(Msg::Palette(PaletteEvent::Ui(PaletteMsg::Query("2^10/4".into()))));
    let s = h.program().palette.as_ref().unwrap();
    assert_eq!(s.sections[0].rows[0].item.title, "= 256");
}

#[test]
fn keys_move_scope_run_and_close() {
    let mut h = scene(false);
    let p = open(&mut h, "");
    h.named(p, NamedKey::Tab, Modifiers::NONE);
    assert_eq!(h.program().palette.as_ref().unwrap().scope, Some(Scope::Command));
    h.named(p, NamedKey::Tab, Modifiers::SHIFT);
    assert_eq!(h.program().palette.as_ref().unwrap().scope, None);
    h.type_text(p, "lock");
    h.named(p, NamedKey::ArrowDown, Modifiers::NONE);
    h.named(p, NamedKey::ArrowUp, Modifiers::NONE);
    assert_eq!(h.program().palette.as_ref().unwrap().selected, 0);
    h.named(p, NamedKey::Escape, Modifiers::NONE);
    assert!(h.program().palette.is_none());
    assert!(h.windows().iter().all(|k| h.title(*k) != "Palette"));
}

#[test]
fn running_a_command_asks_the_app() {
    let mut h = scene(false);
    let p = open(&mut h, "italic");
    h.named(p, NamedKey::Enter, Modifiers::NONE);
    assert!(h.program().palette.is_none(), "running closes it");
    // The pick is remembered for the coach and ranking.
    assert!(h.program().frecency.score("menu:org.aizome.Sumi:Format/Italic") > 0.9);
}

#[test]
fn pictures_of_the_palette() {
    for day in [false, true] {
        let mut h = scene(day);
        let p = open(&mut h, "f");
        {
            // The coach has something to say about Find… after picks.
            let s = h.program_mut();
            for _ in 0..14 {
                s.frecency.visit("menu:org.aizome.Sumi:Format/Bold");
            }
        }
        h.send(Msg::Palette(PaletteEvent::Ui(PaletteMsg::Query("bo".into()))));
        let name = if day { "palette-day.png" } else { "palette-night.png" };
        h.save_png(p, shots().join(name), 2.0);
        aizome_kit::testing::assert_golden(&mut h, p, golden(name), 2.0);
    }
}

/// Two seconds after start the shell warms the palette, and a second later
/// draws it ahead, hidden, as it would open: its first opening then shows
/// that drawing and draws only what changed since (nothing, here), not the
/// whole surface, and what shows is the palette drawn whole. Closed, it's kept hidden and drawn ahead again a second later;
/// a new theme draws it ahead again in that theme.
#[test]
fn the_first_opening_draws_only_what_changed_since_it_was_drawn_ahead() {
    use aizome_shell::palette::AHEAD_AFTER;
    let mut h = scene(false);
    h.send(Msg::WarmPalette);
    assert!(h.windows().iter().all(|k| h.title(*k) != "Palette"), "drawn ahead a second later");
    h.advance(AHEAD_AFTER);
    let p = window_titled(&h, "Palette");
    assert!(!h.is_shown(p), "drawn ahead hidden");
    let size = h.size(p);
    let whole = size.width * size.height;
    let ahead = h.check_frame(p);
    assert!(ahead.damaged >= whole, "drawn ahead whole: {} of {whole} points²", ahead.damaged);
    // Its bands, put together, are the palette drawn whole, pixel for pixel.
    let (w, hgt, drawn) = h.picture(p).expect("drawn ahead");
    let image = h.render(p, 2.0);
    assert_eq!(image.pixel_size(), (w, hgt));
    assert_eq!(aizome_kit::testing::difference(&drawn, &image.to_rgba8().expect("pixels"), 0), 0.0);
    for (round, day) in [(0, false), (1, false), (2, true)] {
        h.send(Msg::Run(Action::Palette { scope: None, query: String::new() }));
        assert_eq!(window_titled(&h, "Palette"), p, "the window drawn ahead shows");
        assert!(h.is_shown(p));
        let frame = h.check_frame(p);
        assert_eq!(frame.differs, 0.0, "round {round}: what shows is the palette drawn whole");
        assert!(
            frame.damaged < whole / 8.0,
            "round {round}: the first frame drew {} of {whole} points², not just what changed",
            frame.damaged
        );
        println!(
            "round {round}: the first opening drew {:.0} of {whole:.0} points² ({:.0} of {:.0} pixels at 2×)",
            frame.damaged,
            frame.damaged * 4.0,
            whole * 4.0
        );
        assert_eq!(h.focused_name(p).as_deref(), Some("palette-query"));
        h.named(p, NamedKey::Escape, Modifiers::NONE);
        assert!(h.program().palette.is_none());
        assert!(!h.is_shown(p), "round {round}: kept, hidden");
        if day {
            break;
        }
        h.advance(AHEAD_AFTER);
        assert!(h.check_frame(p).damaged >= whole, "round {round}: drawn ahead again");
        if round == 1 {
            // A new theme: drawn ahead again, in it.
            h.set_appearance(appearance(true));
            // (At idle: the loop's next turns.)
            h.advance(std::time::Duration::ZERO);
            assert!(h.check_frame(p).damaged >= whole, "drawn ahead again in the new theme");
        }
    }
}
