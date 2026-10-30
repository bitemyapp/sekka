//! What a conversation asks of the kit: text to select and copy, links in
//! it to follow, formulas set in its sentences, a scroll that keeps to the
//! end as text arrives (unless the reader went back) and goes there when
//! asked, a composer as tall as its text that hands Return and pictures to
//! the program, and a side pane that folds away while the other keeps its
//! place.

use aizome_kit::prelude::*;
use aizome_kit::sidestep_ui::{Modifiers, NamedKey};
use aizome_kit::testing::Harness;
use aizome_kit::widgets::layout::column as column_of;
use aizome_kit::{EditorContent, column};

struct Reader {
    lines: usize,
    to_end: u64,
    opened: Vec<String>,
    editor: EditorContent,
    sent: usize,
    pasted: Vec<Vec<String>>,
    read: Option<Vec<u8>>,
    side: bool,
}

#[derive(Clone, Debug)]
enum Msg {
    More,
    ToEnd,
    Open(String),
    Edit(EditorAction),
    Send,
    Pasted(Vec<String>),
    Read(Option<Vec<u8>>),
    Side(bool),
}

impl Program for Reader {
    type Msg = Msg;
    fn info(&self) -> AppInfo {
        AppInfo::new("org.aizome.Reader", "Reader", "Test")
    }
    fn launched(&mut self, cx: &mut Cx<Msg>) {
        cx.open_window(WindowSpec::new("Reader").size(500.0, 400.0));
    }
    fn update(&mut self, msg: Msg, cx: &mut Cx<Msg>) {
        match msg {
            Msg::More => self.lines += 40,
            Msg::ToEnd => self.to_end += 1,
            Msg::Open(target) => {
                cx.open_url(&target);
                self.opened.push(target);
            }
            Msg::Edit(action) => {
                self.editor.perform(action);
            }
            Msg::Send => self.sent += 1,
            Msg::Pasted(types) => {
                cx.read_clipboard("image/png", Msg::Read);
                self.pasted.push(types);
            }
            Msg::Read(bytes) => self.read = bytes,
            Msg::Side(on) => self.side = on,
        }
    }
    fn view(&self, _w: WindowKey) -> Element<Msg> {
        let mut lines: Vec<Element<Msg>> = vec![
            text("Select me, then copy").selectable().into(),
            rich_text([
                Span::new("Read "),
                Span::new("the docs").link("https://example.test/docs"),
                Span::new(" first."),
            ])
            .selectable()
            .on_link(Msg::Open)
            .into(),
        ];
        lines.extend((0..self.lines).map(|i| text(format!("line {i}")).into()));
        let reading = scroll(column_of(lines).width(Fill)).follow_end().scroll_to_end(self.to_end).key("reading");
        let composer = text_editor(&self.editor)
            .fit_lines(1, 3)
            .on_action(Msg::Edit)
            .on_key(|key| {
                (key.pressed
                    && key.key == aizome_kit::sidestep_ui::Key::Named(NamedKey::Enter)
                    && !key.modifiers.shift())
                .then_some(Msg::Send)
            })
            .on_paste(|types| types.iter().any(|t| t.starts_with("image/")).then(|| Msg::Pasted(types.to_vec())))
            .height(Shrink)
            .id("composer");
        let main = column![reading, composer].width(Fill).height(Fill);
        split(container(main).id("main").width(Fill).height(Fill), text("side pane"))
            .second_size(150.0)
            .collapse_second(!self.side)
            .into()
    }
}

fn reader() -> (Harness<Reader>, WindowKey) {
    let mut h = Harness::new(Reader {
        lines: 0,
        to_end: 0,
        opened: Vec::new(),
        editor: EditorContent::new(""),
        sent: 0,
        pasted: Vec::new(),
        read: None,
        side: false,
    });
    let w = h.window();
    // As on the desktop, the window has the keyboard: its menus' Edit items
    // act on what is focused in it.
    h.focus_window(w, true);
    (h, w)
}

fn shown(h: &Harness<Reader>, w: WindowKey, what: &str) -> bool {
    let size = h.size(w);
    h.find(w, what).is_some_and(|r| r.y0 >= 0.0 && r.y1 <= size.height)
}

#[test]
fn text_selects_by_dragging_and_copies() {
    let (mut h, w) = reader();
    let at = h.find(w, "Select me, then copy").expect("the text");
    h.drag(w, Point::new(at.x0 + 1.0, at.center().y), Point::new(at.x1 - 1.0, at.center().y), 6);
    assert_eq!(h.selection(w).as_deref(), Some("Select me, then copy"));
    h.command(w, 'c');
    assert_eq!(h.platform.clipboard.as_deref(), Some("Select me, then copy"));
}

#[test]
fn a_link_in_text_opens_with_a_click() {
    let (mut h, w) = reader();
    // "the docs" runs from about a quarter to two thirds of the line.
    let at = h.find(w, "Read the docs first.").expect("the line");
    h.click(w, Point::new(at.x0 + at.width() * 0.45, at.center().y));
    assert_eq!(h.program().opened, ["https://example.test/docs"]);
    assert_eq!(h.platform.opened, ["https://example.test/docs"], "handed to the desktop");
}

#[test]
fn a_scroll_follows_its_end_until_the_reader_goes_back_and_returns_when_asked() {
    let (mut h, w) = reader();
    h.send(Msg::More);
    assert!(shown(&h, w, "line 39"), "at the end once it overflows");
    h.send(Msg::More);
    assert!(shown(&h, w, "line 79"), "still at the end as more arrives");
    h.scroll(w, Point::new(100.0, 100.0), Vec2::new(0.0, -10_000.0));
    assert!(shown(&h, w, "Select me, then copy"), "the reader went back to the start");
    h.send(Msg::More);
    assert!(!shown(&h, w, "line 119") && shown(&h, w, "Select me, then copy"), "and stays there");
    h.send(Msg::ToEnd);
    assert!(shown(&h, w, "line 119"), "until the program asks for the end");
}

#[test]
fn the_composer_grows_with_its_text_and_hands_over_return_and_pictures() {
    let (mut h, w) = reader();
    let composer = h.find_id(w, "composer").expect("the composer");
    h.click(w, composer.center());
    let one = composer.height();
    h.type_text(w, "one");
    h.named(w, NamedKey::Enter, Modifiers::SHIFT);
    h.type_text(w, "two");
    let two = h.find_id(w, "composer").expect("the composer").height();
    for line in ["three", "four", "five"] {
        h.named(w, NamedKey::Enter, Modifiers::SHIFT);
        h.type_text(w, line);
    }
    let most = h.find_id(w, "composer").expect("the composer").height();
    let step = two - one;
    assert!(step > 10.0, "a line more, a line taller: {one} then {two}");
    assert!((most - one - 2.0 * step).abs() < 1.0, "three lines at most: {most}");
    h.named(w, NamedKey::Enter, Modifiers::NONE);
    assert_eq!(h.program().sent, 1, "Return went to the program");
    assert_eq!(h.program().editor.text(), "one\ntwo\nthree\nfour\nfive", "and not into the text");
    h.platform.clipboard = None;
    h.platform.offer = vec![("image/png".into(), vec![0x89, b'P', b'N', b'G'])];
    h.command(w, 'v');
    assert_eq!(h.program().pasted, [vec!["image/png".to_owned()]]);
    assert_eq!(h.program().read.as_deref(), Some(&[0x89, b'P', b'N', b'G'][..]), "read on a worker");
    h.platform.offer.clear();
    h.platform.clipboard = Some(" six".into());
    h.command(w, 'v');
    assert!(h.program().editor.text().ends_with("five six"), "text still pastes as text");
}

#[test]
fn a_folded_side_pane_gives_the_other_all_the_room() {
    let (mut h, w) = reader();
    let width = h.size(w).width;
    assert!((h.find_id(w, "main").expect("the main pane").width() - width).abs() < 1.0);
    assert!(h.find(w, "side pane").is_none(), "nothing of the folded pane shows");
    h.send(Msg::Side(true));
    let main = h.find_id(w, "main").expect("the main pane").width();
    assert!(main < width - 140.0, "the side pane takes its room: {main} of {width}");
    assert!(h.find(w, "side pane").is_some());
}

/// A sentence with something drawn in it: a formula's box, three ems wide.
struct Sentence {
    painted: std::rc::Rc<std::cell::RefCell<Vec<InlineBox>>>,
}

impl Program for Sentence {
    type Msg = ();
    fn info(&self) -> AppInfo {
        AppInfo::new("org.aizome.Sentence", "Sentence", "Test")
    }
    fn launched(&mut self, cx: &mut Cx<()>) {
        cx.open_window(WindowSpec::new("Sentence").size(500.0, 300.0));
    }
    fn update(&mut self, _: (), _: &mut Cx<()>) {}
    fn view(&self, _w: WindowKey) -> Element<()> {
        let painted = self.painted.clone();
        let formula = Inline::new(3.0, 1.2, 0.5, "$x^2$", 7, move |c, place| {
            painted.borrow_mut().push(place);
            c.fill_rect(place.rect, place.color);
        });
        column![
            rich_text([Span::new("Say "), Span::inline(formula).role(Role::Accent), Span::new(" aloud.")])
                .selectable()
                .id("with"),
            rich_text([Span::new("Say  aloud.")]).id("without"),
        ]
        .into()
    }
}

#[test]
fn something_drawn_in_a_sentence_takes_its_room_and_copies_as_its_text() {
    let painted = std::rc::Rc::default();
    let mut h = Harness::new(Sentence { painted: std::rc::Rc::clone(&painted) });
    let w = h.window();
    h.focus_window(w, true);
    h.advance(std::time::Duration::from_millis(50));
    let em = h.theme().metrics.body;
    let (with, without) = (h.find_id(w, "with").unwrap(), h.find_id(w, "without").unwrap());
    assert!((with.width() - without.width() - 3.0 * em).abs() < 1.0, "three ems more: {with:?} {without:?}");
    assert!(with.height() >= 1.7 * em - 0.5, "as tall as it reaches: {with:?}");
    let (pw, _, px) = h.render_rgba(w, 1.0);
    let place = *painted.borrow().last().expect("drawn");
    let centre = place.rect.center();
    let at = (centre.y as usize * pw as usize + centre.x as usize) * 4;
    let accent = h.theme().role(Role::Accent);
    let want = [accent.r, accent.g, accent.b].map(|c| (c * 255.0).round() as i32);
    assert!((0..3).all(|i| (i32::from(px[at + i]) - want[i]).abs() <= 2), "drawn in its box: {:?}", &px[at..at + 4]);
    assert!((place.em - em).abs() < 0.01 && (place.rect.width() - 3.0 * em).abs() < 0.01, "{place:?}");
    assert!(place.rect.x0 > with.x0 + 5.0 && place.rect.y0 >= with.y0 - 0.5, "{place:?} in {with:?}");
    assert_eq!(place.color, h.theme().role(Role::Accent), "in its span's colour");
    h.drag(w, Point::new(with.x0 + 1.0, with.center().y), Point::new(with.x1 - 1.0, with.center().y), 6);
    h.command(w, 'c');
    assert_eq!(h.platform.clipboard.as_deref(), Some("Say $x^2$ aloud."));
}
