//! Dragging selected text out of the text editor and text fields: a press
//! on the selection that moves a few points drags the text (to copy); one
//! that doesn't move puts the caret there, as any click does.

use aizome_kit::column;
use aizome_kit::prelude::*;
use aizome_kit::sidestep_ui::PointerButton;
use aizome_kit::testing::Harness;
use aizome_kit::{EditorAction, EditorContent};

struct Notes {
    content: EditorContent,
    field: String,
    secret: String,
}

#[derive(Clone, Debug)]
enum Msg {
    Edit(EditorAction),
    Field(String),
    Secret(String),
}

impl Program for Notes {
    type Msg = Msg;
    fn info(&self) -> AppInfo {
        AppInfo::new("org.aizome.TextDrag", "TextDrag", "Test")
    }
    fn launched(&mut self, cx: &mut Cx<Msg>) {
        cx.open_window(WindowSpec::new("TextDrag").size(560.0, 300.0));
    }
    fn update(&mut self, msg: Msg, _cx: &mut Cx<Msg>) {
        match msg {
            Msg::Edit(a) => {
                self.content.perform(a);
            }
            Msg::Field(s) => self.field = s,
            Msg::Secret(s) => self.secret = s,
        }
    }
    fn view(&self, _w: WindowKey) -> Element<Msg> {
        column![
            text_field(&self.field).on_change(Msg::Field).id("field"),
            text_field(&self.secret).password().on_change(Msg::Secret).id("secret"),
            text_editor(&self.content).on_action(Msg::Edit).line_numbers(false).id("editor"),
        ]
        .spacing(8.0)
        .padding(12.0)
        .into()
    }
}

fn start() -> (Harness<Notes>, WindowKey) {
    let notes = Notes {
        content: EditorContent::new("aaaaaaaa bbbbbbbbbbbb cccccccc\nsecond line\n"),
        field: "alpha bravobravo charlie".into(),
        secret: "hunter2 hunter2".into(),
    };
    let mut h = Harness::new(notes);
    let w = h.window();
    h.focus_window(w, true);
    (h, w)
}

/// What a drag of `text` carries: the text's MIME types, to copy.
fn text_drag(text: &str) -> String {
    format!("{:?}", DragSource::new().text(text).actions(true, false))
}

/// A word selected by a double click at `at`.
fn select_word(h: &mut Harness<Notes>, w: WindowKey, at: Point) {
    h.press(w, at, PointerButton::Left, 2);
    h.release(w, at, PointerButton::Left);
}

#[test]
fn the_editors_selection_drags_out_as_text() {
    let (mut h, w) = start();
    let editor = h.find_id(w, "editor").unwrap();
    let on_b = Point::new(editor.x0 + 120.0, editor.y0 + 12.0);
    select_word(&mut h, w, on_b);
    let s = h.program().content.selection();
    let word = h.program().content.text()[s.anchor.min(s.head)..s.anchor.max(s.head)].to_owned();
    assert_eq!(word, "bbbbbbbbbbbb", "the double click picked the middle word");
    // A press on it that moves two points: nothing yet; past four, a drag
    // of the text, and the selection stays.
    h.press(w, on_b, PointerButton::Left, 1);
    h.move_to(w, on_b + Vec2::new(2.0, 0.0));
    assert!(h.drags().is_empty());
    h.move_to(w, on_b + Vec2::new(30.0, 20.0));
    assert_eq!(h.drags().len(), 1);
    assert_eq!(format!("{:?}", h.drags()[0].1), text_drag("bbbbbbbbbbbb"));
    h.release(w, on_b + Vec2::new(30.0, 20.0), PointerButton::Left);
    assert_eq!(h.program().content.selection(), s, "dragging out leaves the selection");
    // A press on it that doesn't move: the caret goes there.
    h.press(w, on_b, PointerButton::Left, 1);
    h.release(w, on_b, PointerButton::Left);
    let c = h.program().content.selection();
    assert_eq!(c.anchor, c.head, "a caret");
    assert!(c.head > s.anchor.min(s.head) && c.head < s.anchor.max(s.head), "inside the word: {c:?}");
    assert_eq!(h.drags().len(), 1);
    // A press off the selection selects by dragging, as ever.
    select_word(&mut h, w, on_b);
    let off = Point::new(editor.x0 + 20.0, editor.y0 + 12.0);
    h.press(w, off, PointerButton::Left, 1);
    h.move_to(w, off + Vec2::new(40.0, 0.0));
    h.release(w, off + Vec2::new(40.0, 0.0), PointerButton::Left);
    assert_eq!(h.drags().len(), 1, "no drag");
    let s = h.program().content.selection();
    assert!(s.anchor != s.head && s.anchor.min(s.head) < 8, "a new selection: {s:?}");
}

#[test]
fn a_fields_selection_drags_out_but_a_passwords_never() {
    let (mut h, w) = start();
    let field = h.find_id(w, "field").unwrap();
    let on = Point::new(field.x0 + 80.0, field.center().y);
    select_word(&mut h, w, on);
    assert_eq!(h.selection(w).as_deref(), Some("bravobravo"));
    h.press(w, on, PointerButton::Left, 1);
    h.move_to(w, on + Vec2::new(10.0, 10.0));
    assert_eq!(h.drags().len(), 1);
    assert_eq!(format!("{:?}", h.drags()[0].1), text_drag("bravobravo"));
    h.release(w, on + Vec2::new(10.0, 10.0), PointerButton::Left);
    assert_eq!(h.selection(w).as_deref(), Some("bravobravo"), "the selection stays");
    // A click on it puts the caret there.
    h.press(w, on, PointerButton::Left, 1);
    h.release(w, on, PointerButton::Left);
    assert_eq!(h.selection(w), None);
    // A password's selection is never dragged out.
    let secret = h.find_id(w, "secret").unwrap();
    let on = Point::new(secret.x0 + 30.0, secret.center().y);
    h.press(w, on, PointerButton::Left, 3);
    h.release(w, on, PointerButton::Left);
    h.press(w, on, PointerButton::Left, 1);
    h.move_to(w, on + Vec2::new(30.0, 0.0));
    h.release(w, on + Vec2::new(30.0, 0.0), PointerButton::Left);
    assert_eq!(h.drags().len(), 1, "no drag of a password");
}
