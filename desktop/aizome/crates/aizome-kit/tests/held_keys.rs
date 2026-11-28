//! A held key presses nothing: Return and Space press a button (the focused
//! one, or a sheet's default) only as the key first goes down. The repeats
//! of a key held since before a button got the focus, or before its alert
//! opened, aren't the person's press of it. Typing and Tab still repeat.

use aizome_kit::prelude::*;
use aizome_kit::sidestep_ui::{Key, Modifiers, NamedKey};
use aizome_kit::testing::Harness;

#[derive(Default)]
struct Trash {
    window: Option<WindowKey>,
    name: String,
    sure: bool,
    /// Times the alert's default button answered.
    emptied: usize,
    /// Times the Empty… button asked.
    asked: usize,
    /// Times the Count button was pressed.
    counted: usize,
}

#[derive(Clone, Debug)]
enum Msg {
    Name(String),
    /// Return in the field: on to the Count button.
    ToCount,
    Sure(bool),
    Ask,
    Empty,
    Count,
}

impl Program for Trash {
    type Msg = Msg;

    fn info(&self) -> AppInfo {
        AppInfo::new("org.aizome.HeldKeysTest", "Held Keys Test", "Test")
    }

    fn launched(&mut self, cx: &mut Cx<Msg>) {
        self.window = Some(cx.open_window(WindowSpec::new("Trash").size(480.0, 320.0)));
    }

    fn update(&mut self, msg: Msg, cx: &mut Cx<Msg>) {
        match msg {
            Msg::Name(name) => self.name = name,
            Msg::ToCount => cx.focus("count"),
            Msg::Sure(on) => self.sure = on,
            Msg::Ask => {
                self.asked += 1;
                cx.alert(
                    self.window.unwrap(),
                    Alert::new("Empty the Trash?", "Its items will be deleted for good.")
                        .default_button("Empty Trash", Msg::Empty)
                        .cancel_button("Cancel"),
                );
            }
            Msg::Empty => self.emptied += 1,
            Msg::Count => self.counted += 1,
        }
    }

    fn view(&self, _: WindowKey) -> Element<Msg> {
        aizome_kit::column![
            text_field(&self.name).on_change(Msg::Name).on_submit(Msg::ToCount).id("name"),
            checkbox("I'm sure", self.sure).on_toggle(Msg::Sure).id("sure"),
            button("Empty…").on_press(Msg::Ask).id("ask"),
            button("Count").on_press(Msg::Count).id("count"),
        ]
        .spacing(12.0)
        .padding(16.0)
        .into()
    }
}

fn trash() -> (Harness<Trash>, WindowKey) {
    let mut h = Harness::new(Trash::default());
    let w = h.window();
    h.focus_window(w, true);
    (h, w)
}

fn focus(h: &mut Harness<Trash>, w: WindowKey, name: &str) {
    for _ in 0..5 {
        if h.focused_name(w).as_deref() == Some(name) {
            return;
        }
        h.named(w, NamedKey::Tab, Modifiers::NONE);
    }
    assert_eq!(h.focused_name(w).as_deref(), Some(name));
}

fn asking(h: &Harness<Trash>, w: WindowKey) -> bool {
    h.texts(w).iter().any(|t| t == "Empty the Trash?")
}

fn enter() -> Key {
    Key::Named(NamedKey::Enter)
}

fn space() -> Key {
    Key::Character(" ".into())
}

/// The key held down after its first press: as the platform repeats it
/// (`marked`), or as presses with no release between, which is what a repeat
/// is from a platform that doesn't mark it.
fn hold(h: &mut Harness<Trash>, w: WindowKey, key: Key, marked: bool) {
    for _ in 0..40 {
        if marked {
            h.key_repeat(w, key.clone(), Modifiers::NONE);
        } else {
            h.key_down(w, key.clone(), Modifiers::NONE);
        }
    }
}

/// A key held on a focused button presses it once, as it goes down.
#[test]
fn a_key_held_on_a_button_presses_it_once() {
    for marked in [true, false] {
        let (mut h, w) = trash();
        focus(&mut h, w, "count");
        for (n, key) in [space(), enter()].into_iter().enumerate() {
            h.key_down(w, key.clone(), Modifiers::NONE);
            hold(&mut h, w, key.clone(), marked);
            h.key_up(w, key.clone(), Modifiers::NONE);
            assert_eq!(h.program().counted, n + 1, "{key:?} held on the button (repeats marked: {marked})");
        }
    }
}

/// Return in a field that moves the focus to a button: still held, it went
/// down before the button had the focus, and is no press of it. Let go and
/// pressed again, it is one.
#[test]
fn a_return_held_since_a_field_presses_nothing_at_the_button_it_moved_to() {
    for marked in [true, false] {
        let (mut h, w) = trash();
        focus(&mut h, w, "name");
        h.key_down(w, enter(), Modifiers::NONE);
        assert_eq!(h.focused_name(w).as_deref(), Some("count"), "Return in the field moves to the button");
        hold(&mut h, w, enter(), marked);
        assert_eq!(h.program().counted, 0, "the held key pressed the button (repeats marked: {marked})");
        h.key_up(w, enter(), Modifiers::NONE);
        assert_eq!(h.program().counted, 0);
        h.named(w, NamedKey::Enter, Modifiers::NONE);
        assert_eq!(h.program().counted, 1, "Return pressed again presses it");
    }
}

/// Return on the button that asks opens the alert; held, its repeats don't
/// answer for the alert's default button. Let go and pressed again, it does.
#[test]
fn a_return_held_on_the_button_that_asks_doesnt_answer_its_alert() {
    for marked in [true, false] {
        let (mut h, w) = trash();
        focus(&mut h, w, "ask");
        h.key_down(w, enter(), Modifiers::NONE);
        assert!(asking(&h, w), "Return on the focused button asks");
        hold(&mut h, w, enter(), marked);
        assert_eq!(h.program().emptied, 0, "the held key answered the alert (repeats marked: {marked})");
        assert!(asking(&h, w), "the alert is still up");
        h.key_up(w, enter(), Modifiers::NONE);
        assert_eq!(h.program().emptied, 0);
        h.named(w, NamedKey::Enter, Modifiers::NONE);
        assert_eq!(h.program().emptied, 1, "Return pressed again answers");
        assert!(!asking(&h, w));
        assert_eq!(h.program().asked, 1);
    }
}

/// A Space held on a checkbox flips it once: what it shows when the key is
/// let go is what the person set.
#[test]
fn a_space_held_on_a_checkbox_flips_it_once() {
    let (mut h, w) = trash();
    focus(&mut h, w, "sure");
    h.key_down(w, space(), Modifiers::NONE);
    for _ in 0..5 {
        h.key_repeat(w, space(), Modifiers::NONE);
    }
    h.key_up(w, space(), Modifiers::NONE);
    assert!(h.program().sure, "held Space flipped the checkbox back and forth");
    h.key(w, space(), Modifiers::NONE);
    assert!(!h.program().sure, "Space pressed again flips it back");
}

/// What a held key does elsewhere is unchanged: it types again and again in
/// a field, and Tab keeps walking the focus.
#[test]
fn typing_and_tab_still_repeat() {
    let (mut h, w) = trash();
    focus(&mut h, w, "name");
    let a = Key::Character("a".into());
    h.key_down(w, a.clone(), Modifiers::NONE);
    h.key_repeat(w, a.clone(), Modifiers::NONE);
    h.key_repeat(w, a.clone(), Modifiers::NONE);
    h.key_up(w, a, Modifiers::NONE);
    assert_eq!(h.program().name, "aaa");
    let tab = Key::Named(NamedKey::Tab);
    h.key_down(w, tab.clone(), Modifiers::NONE);
    assert_eq!(h.focused_name(w).as_deref(), Some("sure"));
    h.key_repeat(w, tab.clone(), Modifiers::NONE);
    assert_eq!(h.focused_name(w).as_deref(), Some("ask"));
    h.key_up(w, tab, Modifiers::NONE);
    assert_eq!(h.program().asked, 0);
}

/// A key that was down when the window lost the keyboard may be let go where
/// the window can't see: it isn't taken for held when the keyboard comes
/// back, and its next press presses.
#[test]
fn a_key_down_when_the_keyboard_left_is_forgotten() {
    let (mut h, w) = trash();
    focus(&mut h, w, "count");
    h.key_down(w, enter(), Modifiers::NONE);
    assert_eq!(h.program().counted, 1);
    h.focus_window(w, false);
    h.focus_window(w, true);
    assert_eq!(h.focused_name(w).as_deref(), Some("count"));
    h.named(w, NamedKey::Enter, Modifiers::NONE);
    assert_eq!(h.program().counted, 2);
}
