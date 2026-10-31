//! Pictures of Nisshi's main views in Night and Day, from a made-up boot
//! (no message on this computer is shown): golden images
//! (`AIZOME_UPDATE_GOLDENS=1` rewrites them).

use aizome_kit::testing::{Harness, assert_golden};
use aizome_kit::theme::{Appearance, ThemeChoice};
use nisshi::app::{Access, Msg, Nisshi};
use nisshi::fixture;

const THEMES: [(ThemeChoice, &str); 2] = [(ThemeChoice::Night, "night"), (ThemeChoice::Day, "day")];

fn harness(theme: ThemeChoice, access: Access) -> Harness<Nisshi> {
    let n =
        Nisshi::sample(fixture::journal(), fixture::boots(), fixture::files(), fixture::clock(), fixture::UID, access);
    Harness::with(n, Appearance { theme, ..Appearance::default() }, false)
}

fn golden(h: &mut Harness<Nisshi>, name: &str, theme: &str) {
    let w = h.window();
    assert_golden(h, w, format!("tests/snapshots/{name}-{theme}.png"), 1.0);
}

/// The seq of the newest message whose text has `needle`.
fn find(h: &Harness<Nisshi>, needle: &str) -> u64 {
    h.program()
        .store
        .entries
        .iter()
        .rev()
        .find(|e| e.message.contains(needle))
        .unwrap_or_else(|| panic!("{needle}"))
        .seq
}

#[test]
fn the_boot_with_what_stands_out() {
    for (theme, name) in THEMES {
        let mut h = harness(theme, Access::System);
        golden(&mut h, "boot", name);
    }
}

#[test]
fn a_message_selected_with_its_suggestions() {
    for (theme, name) in THEMES {
        let mut h = harness(theme, Access::System);
        let seq = find(&h, "CTRL-EVENT-DISCONNECTED");
        h.send(Msg::Select(seq));
        h.advance(std::time::Duration::from_millis(200));
        golden(&mut h, "selected", name);
    }
}

#[test]
fn searching_and_a_suggestion_taken() {
    for (theme, name) in THEMES {
        let mut h = harness(theme, Access::System);
        h.send(Msg::Search("kex".into()));
        let seq = find(&h, "kex_exchange_identification");
        h.send(Msg::Select(seq));
        h.advance(std::time::Duration::from_millis(200));
        golden(&mut h, "search", name);
    }
}

#[test]
fn an_invalid_search_explains_what_to_correct() {
    for (theme, name) in THEMES {
        let mut h = harness(theme, Access::System);
        h.send(Msg::Search("/[/ pid:x level:loud since:soon".into()));
        golden(&mut h, "invalid-search", name);
    }
}

#[test]
fn only_your_own_messages_until_unlocked() {
    for (theme, name) in THEMES {
        // Only what the user may read: their own session's.
        let own: Vec<_> = fixture::journal().into_iter().filter(|e| e.uid == Some(fixture::UID)).collect();
        let n = Nisshi::sample(own, fixture::boots(), fixture::files(), fixture::clock(), fixture::UID, Access::Own);
        let mut h = Harness::with(n, Appearance { theme, ..Appearance::default() }, false);
        golden(&mut h, "locked", name);
    }
}
