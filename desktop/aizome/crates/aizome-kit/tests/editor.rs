//! `text_editor`: editing, moving, selecting, undo, input methods, find
//! hooks, damage by line, and long documents.

use std::sync::Arc;

use aizome_kit::column;
use aizome_kit::prelude::*;
use aizome_kit::sidestep_ui::{Key, Modifiers, NamedKey, PointerButton};
use aizome_kit::testing::Harness;
use aizome_kit::{EditorAction, EditorContent, FindRequest, LineStyle};

struct Sumi {
    content: EditorContent,
    finds: Vec<FindRequest>,
    clicked: Vec<usize>,
    numbers: bool,
    marks: Arc<Vec<std::ops::Range<usize>>>,
    current_mark: Option<usize>,
}

#[derive(Clone, Debug)]
enum Msg {
    Edit(EditorAction),
    Find(FindRequest),
    /// The program moves the caret (found text, Go to Line).
    Goto(usize),
    /// A click on a `[` (a task's box).
    Clicked(usize),
    Marks(Vec<std::ops::Range<usize>>, Option<usize>),
}

impl Sumi {
    fn new(text: &str) -> Sumi {
        Sumi {
            content: EditorContent::new(text),
            finds: Vec::new(),
            clicked: Vec::new(),
            numbers: true,
            marks: Arc::new(Vec::new()),
            current_mark: None,
        }
    }
}

fn markdown() -> aizome_kit::Highlighter {
    Arc::new(|_line, text: &str| {
        let mut out = Vec::new();
        if text.starts_with('#') {
            out.push(LineStyle { range: 0..text.len(), span: Span::new("").role(Role::Accent).weight(600.0) });
        }
        if let Some(a) = text.find('`')
            && let Some(b) = text[a + 1..].find('`')
        {
            out.push(LineStyle { range: a..a + b + 2, span: Span::new("").role(Role::Success) });
        }
        if text.starts_with("- ") {
            out.push(LineStyle { range: 0..2, span: Span::new("").role(Role::Violet) });
        }
        out
    })
}

impl Program for Sumi {
    type Msg = Msg;
    fn info(&self) -> AppInfo {
        AppInfo::new("org.aizome.Sumi", "Sumi", "Text Editor")
    }
    fn launched(&mut self, cx: &mut Cx<Msg>) {
        cx.open_window(WindowSpec::new("architecture.md").size(560.0, 300.0));
    }
    fn update(&mut self, msg: Msg, _cx: &mut Cx<Msg>) {
        match msg {
            Msg::Edit(a) => {
                self.content.perform(a);
            }
            Msg::Find(f) => self.finds.push(f),
            Msg::Goto(at) => self.content.set_selection(aizome_kit::Selection::caret(at)),
            Msg::Clicked(at) => self.clicked.push(at),
            Msg::Marks(marks, current) => {
                self.marks = Arc::new(marks);
                self.current_mark = current;
            }
        }
    }
    fn view(&self, _w: WindowKey) -> Element<Msg> {
        let rope = self.content.rope().clone();
        column![
            text_editor(&self.content)
                .on_click(move |at, _| (rope.get_char(at) == Some('[')).then_some(Msg::Clicked(at)))
                .on_action(Msg::Edit)
                .on_find(Msg::Find)
                .line_numbers(self.numbers)
                .highlighter(markdown(), 0)
                .marks(self.marks.clone(), self.current_mark)
                .id("editor")
        ]
        .into()
    }
}

const NONE: Modifiers = Modifiers::NONE;

fn open(text: &str) -> (Harness<Sumi>, WindowKey) {
    let mut h = Harness::new(Sumi::new(text));
    let w = h.window();
    h.focus_window(w, true);
    let e = h.find_id(w, "editor").unwrap();
    h.click(w, Point::new(e.x1 - 20.0, e.y1 - 10.0));
    (h, w)
}

#[test]
fn typing_newlines_and_undo() {
    let (mut h, w) = open("");
    h.type_text(w, "    fn main() {");
    h.named(w, NamedKey::Enter, NONE);
    h.type_text(w, "x");
    assert_eq!(h.program().content.text(), "    fn main() {\n    x");
    h.command(w, 'z');
    assert_eq!(h.program().content.text(), "    fn main() {\n    ");
    h.command(w, 'z');
    h.command(w, 'z');
    assert_eq!(h.program().content.text(), "");
    h.key(w, Key::Character("Z".into()), Modifiers::SUPER | Modifiers::SHIFT);
    assert_eq!(h.program().content.text(), "    fn main() {");
}

#[test]
fn moving_by_words_lines_and_the_document() {
    let (mut h, w) = open("alpha beta\ngamma delta\nepsilon");
    // The click put the caret at the end.
    assert_eq!(h.program().content.selection().head, h.program().content.len_chars());
    h.named(w, NamedKey::ArrowUp, Modifiers::SUPER);
    assert_eq!(h.program().content.selection().head, 0);
    h.named(w, NamedKey::ArrowRight, Modifiers::ALT);
    assert_eq!(h.program().content.selection().head, 5);
    h.named(w, NamedKey::ArrowDown, NONE);
    assert_eq!(h.program().content.cursor_position(), (1, 5), "down keeps the column");
    h.named(w, NamedKey::ArrowRight, Modifiers::SUPER | Modifiers::SHIFT);
    assert_eq!(h.program().content.selected_text(), " delta");
    h.named(w, NamedKey::Backspace, NONE);
    assert_eq!(h.program().content.text(), "alpha beta\ngamma\nepsilon");
    h.named(w, NamedKey::Backspace, Modifiers::ALT);
    assert_eq!(h.program().content.text(), "alpha beta\n\nepsilon");
    h.named(w, NamedKey::ArrowDown, Modifiers::SUPER);
    h.named(w, NamedKey::Backspace, Modifiers::SUPER);
    assert_eq!(h.program().content.text(), "alpha beta\n\n");
}

#[test]
fn clicks_select_words_and_lines_and_the_clipboard_works() {
    let (mut h, w) = open("one two three\nfour five");
    let e = h.find_id(w, "editor").unwrap();
    let line0 = e.y0 + 8.0 + 10.0;
    // A double click on "two" (past the gutter).
    let x = e.x0 + 52.0 + 8.0 * 4.0 + 6.0;
    h.press(w, Point::new(x, line0), PointerButton::Left, 2);
    h.release(w, Point::new(x, line0), PointerButton::Left);
    assert_eq!(h.selection(w).as_deref(), Some("two"));
    h.command(w, 'c');
    assert_eq!(h.platform.clipboard.as_deref(), Some("two"));
    h.press(w, Point::new(x, line0), PointerButton::Left, 3);
    h.release(w, Point::new(x, line0), PointerButton::Left);
    assert_eq!(h.selection(w).as_deref(), Some("one two three\n"));
    h.command(w, 'v');
    assert_eq!(h.program().content.text(), "twofour five");
}

#[test]
fn input_methods_compose_in_place() {
    let (mut h, w) = open("日");
    h.ime(w, None, "ほん");
    assert_eq!(h.program().content.text(), "日");
    h.ime(w, Some("本"), "");
    assert_eq!(h.program().content.text(), "日本");
}

#[test]
fn find_items_reach_the_program() {
    let (mut h, w) = open("find me");
    h.command(w, 'f');
    h.command(w, 'g');
    h.key(w, Key::Character("G".into()), Modifiers::SUPER | Modifiers::SHIFT);
    h.command(w, 'a');
    h.command(w, 'e');
    assert_eq!(
        h.program().finds,
        [FindRequest::Show, FindRequest::Next, FindRequest::Previous, FindRequest::UseSelection("find me".into())]
    );
}

#[test]
fn typing_redraws_its_line_not_the_editor() {
    let text: String = (0..40).map(|i| format!("line {i} of the document\n")).collect();
    let (mut h, w) = open(&text);
    h.named(w, NamedKey::ArrowUp, Modifiers::SUPER);
    h.take_damage(w);
    h.type_text(w, "x");
    let damage = h.take_damage(w);
    assert!(!damage.is_empty());
    let tall: f64 = damage.iter().map(|d| d.height()).fold(0.0, f64::max);
    assert!(tall <= 2.0 * 21.0 + 2.0, "one line (and the caret): {damage:?}");
}

#[test]
fn search_marks_add_move_and_clear_without_changing_text_or_selection() {
    let (mut h, w) = open("cat Cat caterpillar\nnumber:12 number:34\nplain line");
    let selection = h.program().content.selection();
    h.check_frame(w);
    for marks in [vec![0..3, 4..7, 8..11], std::iter::once(4..7).collect(), std::iter::once(19..28).collect(), vec![]] {
        h.send(Msg::Marks(marks, None));
        let frame = h.check_frame(w);
        assert_eq!(frame.differs, 0.0, "updated marks must match a full redraw: {frame:?}");
        assert!(frame.damaged > 0.0 && frame.damaged <= 560.0 * 42.0, "only affected lines: {frame:?}");
        assert_eq!(h.program().content.selection(), selection);
        assert_eq!(h.program().content.text(), "cat Cat caterpillar\nnumber:12 number:34\nplain line");
    }
}

#[test]
fn current_search_mark_repaints_without_moving_the_selection() {
    let (mut h, w) = open("雪 cat 雪 cat\nunchanged");
    let selection = h.program().content.selection();
    let marks = vec![2..5, 8..11];
    h.send(Msg::Marks(marks.clone(), None));
    h.check_frame(w);
    for current in [Some(0), Some(1), None] {
        h.send(Msg::Marks(marks.clone(), current));
        let frame = h.check_frame(w);
        assert_eq!(frame.differs, 0.0, "current mark must repaint: {frame:?}");
        // The Unicode fallback font can make this line taller than 21pt.
        assert!(frame.damaged > 0.0 && frame.damaged <= 560.0 * 30.0, "one line: {frame:?}");
        assert_eq!(h.program().content.selection(), selection);
    }
}

#[test]
fn offscreen_search_mark_changes_do_not_redraw_visible_lines() {
    let text: String = (0..100).map(|i| format!("line {i}\n")).collect();
    let mut h = Harness::new(Sumi::new(&text));
    let w = h.window();
    h.check_frame(w);
    let at = h.program().content.rope().line_to_char(90);
    h.send(Msg::Marks(std::iter::once(at..at + 4).collect(), Some(0)));
    let frame = h.check_frame(w);
    assert_eq!(frame.differs, 0.0);
    assert_eq!(frame.damaged, 0.0, "offscreen marks leave the visible buffer alone: {frame:?}");
}

#[test]
fn long_documents_lay_out_only_what_shows() {
    let text: String = (0..100_000).map(|i| format!("line {i}\n")).collect();
    let mut h = Harness::new(Sumi::new(&text));
    let w = h.window();
    let started = std::time::Instant::now();
    h.scroll(w, Point::new(200.0, 100.0), Vec2::new(0.0, 210_000.0));
    assert!(started.elapsed() < std::time::Duration::from_secs(5));
    h.focus_window(w, true);
    let e = h.find_id(w, "editor").unwrap();
    h.click(w, Point::new(e.x0 + 100.0, e.y0 + 30.0));
    let (line, _) = h.program().content.cursor_position();
    assert!(line > 9_000, "scrolled far down: line {line}");
    h.named(w, NamedKey::ArrowDown, Modifiers::SUPER);
    h.type_text(w, "end");
    assert!(h.program().content.text().ends_with("end"));
}

#[test]
fn an_editor_as_the_board() {
    let text = "## Scale\n\nOperations stay in points. Each window renders at its\noutput's scale: the integer one wherever it can, the\nbuffer of `points × scale` pixels.\n\n### Damage\nA canvas fills only the pixels whose centres its\ndamage contains, so neighbouring fills share edges\n\n- Clock tick: one rect, the clock's own.\n- Caret blink: two points wide, one line tall.\n";
    let mut h = Harness::new(Sumi::new(text));
    let w = h.window();
    h.focus_window(w, true);
    let e = h.find_id(w, "editor").unwrap();
    // Select from "fills" on line 8 into line 9.
    h.press(w, Point::new(e.x0 + 52.0 + 8.0 * 9.0, e.y0 + 8.0 + 21.0 * 7.0 + 10.0), PointerButton::Left, 1);
    h.drag(
        w,
        Point::new(e.x0 + 52.0 + 8.0 * 9.0, e.y0 + 8.0 + 21.0 * 7.0 + 10.0),
        Point::new(e.x0 + 52.0 + 8.0 * 15.0, e.y0 + 8.0 + 21.0 * 8.0 + 10.0),
        4,
    );
    let out = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("editor.png");
    h.save_png(w, &out, 2.0);
    assert!(h.selection(w).is_some());
}

#[test]
fn the_programs_jumps_are_shown_with_what_comes_before() {
    let text: String = (0..2_000).map(|i| format!("line {i}\n")).collect();
    let (mut h, w) = open(&text);
    // Far down: the line comes a third of the way down the view.
    let at = h.program().content.rope().line_to_char(1_000);
    h.send(Msg::Goto(at));
    let e = h.find_id(w, "editor").unwrap();
    h.click(w, Point::new(e.x0 + 60.0, e.y0 + 12.0 + 4.0));
    let (top, _) = h.program().content.cursor_position();
    let rows = ((e.height() - 24.0) / 21.0) as usize;
    assert_eq!(top, 1_000 - rows / 3, "the line at the top");
    // Near: no jump.
    let at = h.program().content.rope().line_to_char(top + 2);
    h.send(Msg::Goto(at));
    h.click(w, Point::new(e.x0 + 60.0, e.y0 + 12.0 + 4.0));
    assert_eq!(h.program().content.cursor_position().0, top);
}

#[test]
fn clicks_the_program_takes_leave_the_caret() {
    let (mut h, w) = open("- [ ] milk\n- [x] eggs");
    let before = h.program().content.selection();
    let e = h.find_id(w, "editor").unwrap();
    // The text starts 44pt in (the gutter); "- " is two characters.
    let char_w = 7.8;
    h.click(w, Point::new(e.x0 + 44.0 + 2.0 * char_w + 2.0, e.y0 + 12.0 + 10.0));
    assert_eq!(h.program().clicked, [2]);
    assert_eq!(h.program().content.selection(), before, "the caret stayed");
    h.click(w, Point::new(e.x0 + 44.0 + 6.0 * char_w + 2.0, e.y0 + 12.0 + 10.0));
    assert_eq!(h.program().clicked, [2], "elsewhere a click places the caret");
    assert_eq!(h.program().content.selection().head, 6);
}
