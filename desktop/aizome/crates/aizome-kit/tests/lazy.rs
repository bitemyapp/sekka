//! Lazy elements: a subtree the view says is as it was is kept (its
//! function isn't called, its nodes stay), and a window with kept subtrees
//! draws, lays out and answers input as the same window built whole.
//!
//! Most of these run one program two ways, its rows as lazy elements and
//! built outright, through the same steps: after each, the two windows
//! show the same pixels, and the frame the lazy one makes of its damage is
//! the frame drawn whole.

use std::cell::Cell;

use aizome_kit::hash::PaintHasher;
use aizome_kit::layout::{Constraints, Point, Rect, Size};
use aizome_kit::prelude::*;
use aizome_kit::sidestep_ui::{Modifiers, NamedKey};
use aizome_kit::testing::Harness;
use aizome_kit::theme::ThemeChoice;
use aizome_kit::widget::{Event, EventCx, LayoutCx, PaintCx, Status, Widget};
use aizome_kit::widgets::layout::column;
use aizome_kit::{Appearance, column as col};

thread_local! {
    /// Calls of [`row`] on this thread: each test's own count.
    static BUILT: Cell<usize> = const { Cell::new(0) };
}

fn built() -> usize {
    BUILT.with(Cell::get)
}

#[derive(Clone, Debug, PartialEq)]
struct Row {
    id: u32,
    label: String,
    on: bool,
    note: String,
    more: bool,
}

impl Row {
    fn new(id: u32) -> Row {
        Row { id, label: format!("Row {id}"), on: false, note: String::new(), more: false }
    }
}

struct Rows {
    rows: Vec<Row>,
    title: String,
    /// The rows as lazy elements, or built outright.
    lazy: bool,
    keyed: bool,
    roomy: bool,
    window: Option<WindowKey>,
    sheet: Option<WindowKey>,
}

#[derive(Clone, Debug)]
enum Msg {
    Title(String),
    Label(u32, String),
    Toggle(u32, bool),
    Note(u32, String),
    Up(u32),
    Remove(u32),
    Add(u32),
    Roomy(bool),
    More(u32, bool),
    Sheet(bool),
    Again,
}

/// A row, from its value alone: what a lazy element builds.
fn row(r: &Row) -> Element<Msg> {
    BUILT.with(|b| b.set(b.get() + 1));
    let id = r.id;
    aizome_kit::row![
        text(r.label.clone()).width(110.0),
        text_field(&r.note).on_change(move |s| Msg::Note(id, s)).id(format!("note{id}")).width(140.0),
        toggle(r.on).on_toggle(move |on| Msg::Toggle(id, on)).id(format!("on{id}")),
        button("Up").on_press(Msg::Up(id)).id(format!("up{id}")),
        popover(
            button("More").on_press(Msg::More(id, true)).id(format!("more{id}")),
            container(text(format!("More about {}", r.label))).padding(12.0),
            r.more,
        )
        .on_dismiss(Msg::More(id, false)),
        Element::new(Again, Vec::new()).id(format!("again{id}")),
    ]
    .spacing(8.0)
    .padding([2.0, 8.0])
    .into()
}

/// A square that asks for the window to be viewed again when pressed, as
/// a widget does whose own state changes what it is.
struct Again;

impl Widget for Again {
    fn name(&self) -> &'static str {
        "again"
    }

    fn layout(&mut self, _cx: &mut LayoutCx, bc: Constraints) -> Size {
        bc.constrain(Size::new(14.0, 14.0))
    }

    fn paint(&self, cx: &mut PaintCx) {
        let r = cx.bounds();
        cx.canvas.fill_rect(r, Color::hex(0x357d74));
    }

    fn hash(&self, h: &mut PaintHasher) {
        h.add(&1u8);
    }

    fn event(&mut self, cx: &mut EventCx, event: &Event) -> Status {
        match event {
            Event::PointerDown { .. } => {
                cx.rebuild();
                Status::Handled
            }
            _ => Status::Ignored,
        }
    }

    fn hit(&self) -> bool {
        true
    }
}

impl Program for Rows {
    type Msg = Msg;

    fn info(&self) -> AppInfo {
        AppInfo::new("org.aizome.LazyTest", "Lazy Test", "Test")
    }

    fn launched(&mut self, cx: &mut Cx<Msg>) {
        self.window = Some(cx.open_window(WindowSpec::new("Rows").size(560.0, 360.0)));
    }

    fn update(&mut self, msg: Msg, cx: &mut Cx<Msg>) {
        let at = |rows: &[Row], id: u32| rows.iter().position(|r| r.id == id);
        match msg {
            Msg::Title(s) => self.title = s,
            Msg::Label(id, s) => self.rows.iter_mut().filter(|r| r.id == id).for_each(|r| r.label = s.clone()),
            Msg::Toggle(id, on) => self.rows.iter_mut().filter(|r| r.id == id).for_each(|r| r.on = on),
            Msg::Note(id, s) => self.rows.iter_mut().filter(|r| r.id == id).for_each(|r| r.note = s.clone()),
            Msg::More(id, open) => self.rows.iter_mut().filter(|r| r.id == id).for_each(|r| r.more = open),
            Msg::Up(id) => {
                if let Some(i) = at(&self.rows, id).filter(|i| *i > 0) {
                    self.rows.swap(i - 1, i);
                }
            }
            Msg::Remove(id) => self.rows.retain(|r| r.id != id),
            Msg::Add(id) => self.rows.insert(0, Row::new(id)),
            Msg::Roomy(on) => self.roomy = on,
            Msg::Sheet(true) => self.sheet = Some(cx.show_sheet(self.window.expect("a window"))),
            Msg::Sheet(false) => {
                if let Some(s) = self.sheet.take() {
                    cx.close_window(s);
                }
            }
            Msg::Again => {}
        }
    }

    fn view(&self, window: WindowKey) -> Element<Msg> {
        if Some(window) == self.sheet {
            return col![text("A sheet over the rows"), button("Close").on_press(Msg::Sheet(false)).id("close")]
                .spacing(10.0)
                .padding(20.0)
                .into();
        }
        let rows: Vec<Element<Msg>> = self
            .rows
            .iter()
            .map(|r| {
                let el = if self.lazy { lazy(r.clone(), row) } else { row(r) };
                if self.keyed { el.key(r.id) } else { el }
            })
            .collect();
        col![
            text(format!("{} rows", self.rows.len())),
            text_field(&self.title).on_change(Msg::Title).id("title"),
            container(scroll(column(rows)).id("rows")).padding(if self.roomy { 24.0 } else { 4.0 }),
        ]
        .spacing(6.0)
        .padding(10.0)
        .into()
    }
}

fn program(rows: u32, lazy: bool, keyed: bool) -> Rows {
    Rows {
        rows: (0..rows).map(Row::new).collect(),
        title: String::new(),
        lazy,
        keyed,
        roomy: false,
        window: None,
        sheet: None,
    }
}

fn open(rows: u32, lazy: bool, keyed: bool) -> (Harness<Rows>, WindowKey) {
    aizome_kit::fonts::install();
    let mut h = Harness::new(program(rows, lazy, keyed));
    let w = h.window();
    h.focus_window(w, true);
    (h, w)
}

/// The middle of the element named `name`.
fn at(h: &Harness<Rows>, w: WindowKey, name: &str) -> Point {
    h.find_id(w, name).unwrap_or_else(|| panic!("an element named {name}")).center()
}

/// The frame made of the damage since the last one is the window drawn
/// whole: nothing that changed was left as it was.
#[track_caller]
fn frame_is_whole(h: &mut Harness<Rows>, w: WindowKey, after: &str) {
    let check = h.check_frame_exact(w, 1.0);
    assert_eq!(check.differing_pixels, 0, "after {after}: {check:?}");
}

type Step = (&'static str, Box<dyn Fn(&mut Harness<Rows>, WindowKey)>);

fn step(name: &'static str, f: impl Fn(&mut Harness<Rows>, WindowKey) + 'static) -> Step {
    (name, Box::new(f))
}

fn send(name: &'static str, msg: Msg) -> Step {
    step(name, move |h, _| h.send(msg.clone()))
}

/// Run `steps` on the program with lazy rows and with rows built outright:
/// the two show the same after each, and the lazy one's frames are whole.
fn same_either_way(rows: u32, keyed: bool, steps: Vec<Step>) {
    let (mut lazy, lw) = open(rows, true, keyed);
    let (mut plain, pw) = open(rows, false, keyed);
    frame_is_whole(&mut lazy, lw, "opening");
    for (name, act) in steps {
        act(&mut lazy, lw);
        act(&mut plain, pw);
        let (a, b) = (lazy.render_rgba(lw, 1.0), plain.render_rgba(pw, 1.0));
        assert_eq!((a.0, a.1), (b.0, b.1), "after {name}: the windows' sizes");
        let differing = a.2.chunks(4).zip(b.2.chunks(4)).filter(|(x, y)| x != y).count();
        assert_eq!(differing, 0, "after {name}: pixels that differ between lazy rows and rows built outright");
        let (mut ta, mut tb) = (lazy.texts(lw), plain.texts(pw));
        ta.sort();
        tb.sort();
        assert_eq!(ta, tb, "after {name}: the texts");
        assert_eq!(lazy.focused_name(lw), plain.focused_name(pw), "after {name}: the focus");
        assert_eq!(lazy.popups(lw).len(), plain.popups(pw).len(), "after {name}: the popups");
        // The frame each makes of its damage is the frame drawn whole.
        let built_outright = plain.check_frame_exact(pw, 1.0).differing_pixels;
        let check = lazy.check_frame_exact(lw, 1.0);
        assert_eq!(check.differing_pixels, 0, "after {name}: {check:?} ({built_outright} with rows built outright)");
    }
}

#[test]
fn a_view_of_what_has_not_changed_calls_no_lazy_function() {
    let (mut h, w) = open(12, true, false);
    assert_eq!(built(), 12, "each row once, to open");
    assert_eq!(h.lazies().built, 12);
    let widgets = h.widgets(w);

    // Messages that change nothing the rows are built from: no row's
    // function runs, by its own count and by the kit's.
    for (n, title) in ["a", "ab", "abc"].into_iter().enumerate() {
        h.send(Msg::Title(title.into()));
        assert_eq!(built(), 12, "after {title}");
        assert_eq!(h.lazies().built, 12);
        assert_eq!(h.lazies().kept, 12 * (n as u64 + 1));
    }
    h.send(Msg::Again);
    assert_eq!(built(), 12);

    // One row's value changed: that row is built, alone.
    h.send(Msg::Label(5, "Row five".into()));
    assert_eq!(built(), 13);
    assert_eq!(h.lazies().built, 13);
    assert!(h.texts(w).contains(&"Row five".to_owned()));
    assert!(!h.texts(w).contains(&"Row 5".to_owned()));
    // The same value given again is the same.
    h.send(Msg::Label(5, "Row five".into()));
    assert_eq!(built(), 13);
    assert_eq!(h.widgets(w), widgets, "no node came or went");
}

#[test]
fn a_kept_row_is_not_damaged_and_a_changed_one_is_alone() {
    let (mut h, w) = open(8, true, false);
    let _ = h.check_frame_exact(w, 1.0);
    h.send(Msg::Again);
    assert_eq!(h.take_damage(w), Vec::<Rect>::new(), "nothing changed, nothing drawn");

    let switch = h.find_id(w, "on3").expect("row 3's switch");
    let row = Rect::new(0.0, switch.y0 - 8.0, 560.0, switch.y1 + 8.0);
    h.send(Msg::Toggle(3, true));
    let damage = h.take_damage(w);
    assert!(!damage.is_empty());
    for r in &damage {
        assert!(row.contains_rect(*r), "damage {r:?} outside row 3 ({row:?})");
    }
}

#[test]
fn lazy_rows_show_what_rows_built_outright_show() {
    let resize = |size: Size| move |h: &mut Harness<Rows>, w: WindowKey| h.resize(w, size);
    let click = |name: &'static str| move |h: &mut Harness<Rows>, w: WindowKey| h.click(w, at(h, w, name));
    let hover = |name: &'static str| move |h: &mut Harness<Rows>, w: WindowKey| h.move_to(w, at(h, w, name));
    let wheel = |by: f64| move |h: &mut Harness<Rows>, w: WindowKey| h.scroll(w, at(h, w, "rows"), Vec2::new(0.0, by));
    same_either_way(
        30,
        true,
        vec![
            send("a message that changes nothing", Msg::Again),
            send("a title", Msg::Title("Rows".into())),
            send("a switch", Msg::Toggle(2, true)),
            send("a longer label", Msg::Label(4, "The fourth row".into())),
            step("the pointer over a button", hover("up3")),
            send("a message under the pointer", Msg::Again),
            step("a click on a switch", click("on1")),
            step("a click in a field", click("note2")),
            step("typing in it", |h, w| h.type_text(w, "kept")),
            send("a message while it has the focus", Msg::Title("Rows!".into())),
            step("typing on", |h, w| h.type_text(w, " there")),
            send("a row moved up", Msg::Up(2)),
            step("typing in the moved row", |h, w| h.type_text(w, "!")),
            send("a row taken out", Msg::Remove(0)),
            send("a row put in", Msg::Add(100)),
            send("more room around the rows", Msg::Roomy(true)),
            step("a narrower window", resize(Size::new(470.0, 300.0))),
            send("a message after the resize", Msg::Again),
            step("the wheel", wheel(60.0)),
            send("a switch out of sight", Msg::Toggle(1, false)),
            send("a switch in sight", Msg::Toggle(6, true)),
            step("the wheel back", wheel(-25.0)),
            send("a popover opened", Msg::More(5, true)),
            send("a message while it's open", Msg::Title("Rows?".into())),
            send("another row's label while it's open", Msg::Label(7, "Seven".into())),
            send("its own row's label while it's open", Msg::Label(5, "Five".into())),
            send("the popover closed", Msg::More(5, false)),
            send("a sheet", Msg::Sheet(true)),
            send("a row changed under the sheet", Msg::Toggle(4, true)),
            step("a click on a switch under the sheet", click("on3")),
            send("the sheet closed", Msg::Sheet(false)),
            step("a click on the switch", click("on3")),
            step("Day", |h, _| h.set_appearance(Appearance { theme: ThemeChoice::Day, ..Appearance::default() })),
            send("a message in Day", Msg::Toggle(3, false)),
            step("a wider window", resize(Size::new(640.0, 420.0))),
            send("less room", Msg::Roomy(false)),
        ],
    );
}

#[test]
fn rows_without_keys_show_the_same_as_rows_come_and_go() {
    same_either_way(
        10,
        false,
        vec![
            send("a row moved up", Msg::Up(3)),
            send("a row taken out", Msg::Remove(1)),
            send("a row put in", Msg::Add(50)),
            send("a switch", Msg::Toggle(7, true)),
            send("nothing", Msg::Again),
        ],
    );
}

#[test]
fn focus_and_typing_stay_in_a_kept_row() {
    let (mut h, w) = open(6, true, true);
    h.click(w, at(&h, w, "note2"));
    assert_eq!(h.focused_name(w).as_deref(), Some("note2"));
    h.type_text(w, "ab");
    let before = built();
    // The row is kept through messages about other things, with the caret
    // in its field where it was.
    h.send(Msg::Title("x".into()));
    h.send(Msg::Toggle(4, true));
    assert_eq!(built(), before + 1, "row 4 alone");
    assert_eq!(h.focused_name(w).as_deref(), Some("note2"));
    h.named(w, NamedKey::ArrowLeft, Modifiers::NONE);
    h.type_text(w, "c");
    assert_eq!(h.program().rows[2].note, "acb", "typed at the caret, in the row that has it");
    // Its messages still reach the program from a row that was kept.
    h.send(Msg::Again);
    h.click(w, at(&h, w, "on1"));
    assert!(h.program().rows[1].on);
    frame_is_whole(&mut h, w, "typing in a kept row");
}

#[test]
fn the_pointer_s_hover_and_press_stay_on_a_kept_row() {
    let (mut h, w) = open(6, true, false);
    let up = at(&h, w, "up3");
    h.move_to(w, up);
    frame_is_whole(&mut h, w, "hovering");
    let before = built();
    h.send(Msg::Title("x".into()));
    assert_eq!(built(), before);
    frame_is_whole(&mut h, w, "a message under the pointer");
    // Pressed across a message, released on it: it's pressed still.
    h.press(w, up, aizome_kit::sidestep_ui::PointerButton::Left, 1);
    h.send(Msg::Title("xy".into()));
    frame_is_whole(&mut h, w, "a message while pressed");
    h.release(w, up, aizome_kit::sidestep_ui::PointerButton::Left);
    assert_eq!(h.program().rows[2].id, 3, "the press moved row 3 up");
    frame_is_whole(&mut h, w, "the release");
}

#[test]
fn a_scroll_inside_a_kept_subtree_stays_where_it_was() {
    aizome_kit::fonts::install();
    struct Lists {
        lines: u32,
        ticks: u32,
    }
    #[derive(Clone, Debug)]
    enum M {
        Tick,
        Lines(u32),
    }
    fn lines(n: &u32) -> Element<M> {
        BUILT.with(|b| b.set(b.get() + 1));
        scroll(column((0..*n).map(|i| text(format!("line {i}")).into()).collect())).height(120.0).id("lines").into()
    }
    impl Program for Lists {
        type Msg = M;
        fn info(&self) -> AppInfo {
            AppInfo::new("org.aizome.LazyScroll", "Lazy Scroll", "Test")
        }
        fn launched(&mut self, cx: &mut Cx<M>) {
            cx.open_window(WindowSpec::new("Lines").size(300.0, 240.0));
        }
        fn update(&mut self, msg: M, _cx: &mut Cx<M>) {
            match msg {
                M::Tick => self.ticks += 1,
                M::Lines(n) => self.lines = n,
            }
        }
        fn view(&self, _w: WindowKey) -> Element<M> {
            col![text(format!("tick {}", self.ticks)), lazy(self.lines, lines)].padding(10.0).into()
        }
    }
    let mut h = Harness::new(Lists { lines: 60, ticks: 0 });
    let w = h.window();
    let middle = h.find_id(w, "lines").expect("the scroll").center();
    let first = h.find(w, "line 7").expect("a line in sight");
    h.scroll(w, middle, Vec2::new(0.0, 75.0));
    let scrolled = h.find(w, "line 7").expect("the line");
    assert_eq!(scrolled.y0, first.y0 - 75.0);
    let check = h.check_frame_exact(w, 1.0);
    assert_eq!(check.differing_pixels, 0, "{check:?}");

    let before = built();
    h.send(M::Tick);
    h.send(M::Tick);
    assert_eq!(built(), before, "the lines weren't built again");
    assert_eq!(h.find(w, "line 7").expect("the line").y0, scrolled.y0, "nor scrolled back");
    let check = h.check_frame_exact(w, 1.0);
    assert_eq!(check.differing_pixels, 0, "{check:?}");

    // Scrolled on after messages, and built again with another value: the
    // offset is the widget's state, which the new widget takes up.
    h.scroll(w, middle, Vec2::new(0.0, 20.0));
    assert_eq!(h.find(w, "line 7").expect("the line").y0, first.y0 - 95.0);
    h.send(M::Lines(80));
    assert_eq!(built(), before + 1);
    assert_eq!(h.find(w, "line 7").expect("the line").y0, first.y0 - 95.0);
    assert!(h.texts(w).contains(&"line 79".to_owned()));
    let check = h.check_frame_exact(w, 1.0);
    assert_eq!(check.differing_pixels, 0, "{check:?}");
}

#[test]
fn a_kept_row_is_laid_out_again_for_the_room_it_is_given_without_its_function() {
    let (mut h, w) = open(6, true, false);
    let before = built();
    let was = h.find_id(w, "rows").expect("the rows");
    let switch = h.find_id(w, "on2").expect("a switch");

    // More room around the rows: every row moves in, none is built.
    h.send(Msg::Roomy(true));
    assert_eq!(built(), before);
    let now = h.find_id(w, "rows").expect("the rows");
    assert_eq!((now.x0, now.y0), (was.x0 + 20.0, was.y0 + 20.0));
    let moved = h.find_id(w, "on2").expect("a switch");
    assert_eq!((moved.x0, moved.y0), (switch.x0 + 20.0, switch.y0 + 20.0));
    frame_is_whole(&mut h, w, "more room");

    // A window too narrow for a row as it was: the row is laid out in the
    // width it has, and still not built.
    h.resize(w, Size::new(300.0, 360.0));
    assert_eq!(built(), before);
    let narrow = h.find_id(w, "rows").expect("the rows");
    assert!(narrow.width() < now.width() - 100.0, "{narrow:?} from {now:?}");
    frame_is_whole(&mut h, w, "a narrower window");
    h.resize(w, Size::new(560.0, 360.0));
    h.send(Msg::Roomy(false));
    assert_eq!(built(), before);
    let back = h.find_id(w, "on2").expect("a switch");
    assert_eq!((back.x0, back.y0), (switch.x0, switch.y0));
    frame_is_whole(&mut h, w, "the room it had");
}

#[test]
fn keyed_lazy_rows_move_with_their_nodes_and_unkeyed_ones_are_built_where_they_land() {
    // With keys, a row that moves is the same element in another place:
    // nothing is built, and the focus goes with it.
    let (mut h, w) = open(6, true, true);
    h.click(w, at(&h, w, "note3"));
    h.type_text(w, "three");
    let before = built();
    let (two, three) = (h.find_id(w, "on2").expect("row 2"), h.find_id(w, "on3").expect("row 3"));
    h.send(Msg::Up(3));
    assert_eq!(built(), before, "moved, not built");
    assert_eq!(h.find_id(w, "on3").expect("row 3").y0, two.y0);
    assert_eq!(h.find_id(w, "on2").expect("row 2").y0, three.y0);
    assert_eq!(h.focused_name(w).as_deref(), Some("note3"));
    h.type_text(w, "!");
    assert_eq!(h.program().rows[2].note, "three!");
    frame_is_whole(&mut h, w, "a keyed row moved");
    // One taken out: the rest stay as they are.
    let before = built();
    h.send(Msg::Remove(0));
    assert_eq!(built(), before);
    assert!(h.find_id(w, "on0").is_none());
    frame_is_whole(&mut h, w, "a keyed row taken out");

    // Without keys a row is known by its place: the two places that
    // changed hands are built, and no more.
    let (mut h, w) = open(6, true, false);
    let before = built();
    h.send(Msg::Up(3));
    assert_eq!(built(), before + 2);
    frame_is_whole(&mut h, w, "an unkeyed row moved");
    // Taking the first out moves every row up a place.
    let before = built();
    h.send(Msg::Remove(0));
    assert_eq!(built(), before + 5);
    assert!(h.find_id(w, "on0").is_none(), "a name goes with its element");
    frame_is_whole(&mut h, w, "an unkeyed row taken out");
}

#[test]
fn two_lazy_rows_with_one_key_are_each_kept_in_their_place() {
    aizome_kit::fonts::install();
    struct Twins(Vec<&'static str>);
    fn twin(name: &&'static str) -> Element<()> {
        BUILT.with(|b| b.set(b.get() + 1));
        text(*name).into()
    }
    impl Program for Twins {
        type Msg = ();
        fn info(&self) -> AppInfo {
            AppInfo::new("org.aizome.LazyTwins", "Lazy Twins", "Test")
        }
        fn launched(&mut self, cx: &mut Cx<()>) {
            cx.open_window(WindowSpec::new("Twins").size(200.0, 120.0));
        }
        fn update(&mut self, _: (), _cx: &mut Cx<()>) {}
        fn view(&self, _w: WindowKey) -> Element<()> {
            column(self.0.iter().map(|n| lazy(*n, twin).key("twin")).collect()).into()
        }
    }
    let mut h = Harness::new(Twins(vec!["a", "b", "c"]));
    let w = h.window();
    assert_eq!(h.texts(w), ["a", "b", "c"]);
    let before = built();
    h.send(());
    h.send(());
    assert_eq!(built(), before, "each found where it was");
    assert_eq!(h.widgets(w), 10, "the three, what each shows, and the window around them");
    // One key, so place tells them apart: a change of order is a change
    // of value in each place.
    h.program_mut().0 = vec!["b", "a", "c"];
    h.send(());
    assert_eq!(built(), before + 2);
    let mut texts = h.texts(w);
    texts.sort();
    assert_eq!(texts, ["a", "b", "c"]);
    let check = h.check_frame_exact(w, 1.0);
    assert_eq!(check.differing_pixels, 0, "{check:?}");
}

#[test]
fn a_sheet_over_kept_rows_takes_the_input_and_gives_it_back() {
    let (mut h, w) = open(6, true, false);
    let switch = at(&h, w, "on2");
    let before = built();
    h.send(Msg::Sheet(true));
    assert_eq!(built(), before, "the rows under the sheet are kept");
    // The sheet has the input: a click where a switch is doesn't reach it,
    // however many views come and go under the sheet.
    h.click(w, switch);
    h.send(Msg::Title("under".into()));
    h.click(w, switch);
    assert!(!h.program().rows[2].on);
    frame_is_whole(&mut h, w, "a sheet");
    // A row changed under the sheet is built, and shows once it's gone.
    h.send(Msg::Label(2, "Changed under".into()));
    assert_eq!(built(), before + 1);
    let close = at(&h, w, "close");
    h.click(w, close);
    assert!(h.program().sheet.is_none());
    assert!(h.texts(w).contains(&"Changed under".to_owned()));
    h.click(w, switch);
    assert!(h.program().rows[2].on, "the rows take clicks again");
    frame_is_whole(&mut h, w, "the sheet closed");
}

#[test]
fn a_popover_in_a_kept_row_stays_open_and_closes_with_its_value() {
    let (mut h, w) = open(6, true, false);
    h.click(w, at(&h, w, "more2"));
    assert!(h.program().rows[2].more);
    assert_eq!(h.popups(w).len(), 1);
    let popup = h.popups(w)[0];
    assert!(h.texts_in(popup.0).contains(&"More about Row 2".to_owned()));
    // Kept with its row through other messages, where it was.
    let before = built();
    h.send(Msg::Title("x".into()));
    h.send(Msg::Toggle(4, true));
    assert_eq!(built(), before + 1);
    assert_eq!(h.popups(w), vec![popup]);
    // Its row built again with it open: it shows what the row now says.
    h.send(Msg::Label(2, "Second".into()));
    assert_eq!(h.popups(w).len(), 1);
    assert!(h.texts_in(h.popups(w)[0].0).contains(&"More about Second".to_owned()));
    // Closed by its value.
    h.send(Msg::More(2, false));
    assert!(h.popups(w).is_empty());
    // And the row taken out while it's open takes it along.
    h.send(Msg::More(3, true));
    assert_eq!(h.popups(w).len(), 1);
    h.send(Msg::Remove(3));
    assert!(h.popups(w).is_empty());
}

#[test]
fn a_widget_that_asks_for_a_rebuild_has_the_lazy_elements_around_it_built() {
    let (mut h, w) = open(6, true, false);
    h.send(Msg::Again);
    let before = built();
    // The value is the same; the widget asked.
    h.click(w, at(&h, w, "again4"));
    assert_eq!(built(), before + 1, "row 4, and no other");
    h.send(Msg::Again);
    assert_eq!(built(), before + 1, "once: kept again after");
    frame_is_whole(&mut h, w, "a rebuild a widget asked for");
}

#[test]
fn a_new_theme_or_scale_builds_and_draws_everything_again() {
    let (mut h, w) = open(6, true, false);
    let _ = h.check_frame_exact(w, 1.0);
    let before = built();
    h.set_appearance(Appearance { theme: ThemeChoice::Day, ..Appearance::default() });
    assert_eq!(built(), before + 6, "nothing laid out for another theme is kept");
    frame_is_whole(&mut h, w, "Day");
    h.send(Msg::Again);
    assert_eq!(built(), before + 6, "kept again in the new theme");
    h.output_scale(w, 1.5);
    assert_eq!(built(), before + 12, "nor for another scale");
    h.send(Msg::Toggle(1, true));
    assert_eq!(built(), before + 13);
}

#[test]
fn lazy_elements_inside_one_another_are_kept_apart() {
    aizome_kit::fonts::install();
    thread_local! {
        static GROUPS: Cell<usize> = const { Cell::new(0) };
    }
    #[derive(Clone, PartialEq)]
    struct Group {
        name: String,
        items: Vec<u32>,
    }
    struct Groups(Vec<Group>);
    fn item(n: &u32) -> Element<()> {
        BUILT.with(|b| b.set(b.get() + 1));
        text(format!("item {n}")).into()
    }
    fn group(g: &Group) -> Element<()> {
        GROUPS.with(|b| b.set(b.get() + 1));
        let mut all: Vec<Element<()>> = vec![text(g.name.clone()).bold().into()];
        all.extend(g.items.iter().map(|n| lazy(*n, item)));
        column(all).into()
    }
    impl Program for Groups {
        type Msg = ();
        fn info(&self) -> AppInfo {
            AppInfo::new("org.aizome.LazyGroups", "Lazy Groups", "Test")
        }
        fn launched(&mut self, cx: &mut Cx<()>) {
            cx.open_window(WindowSpec::new("Groups").size(240.0, 300.0));
        }
        fn update(&mut self, _: (), _cx: &mut Cx<()>) {}
        fn view(&self, _w: WindowKey) -> Element<()> {
            column(self.0.iter().map(|g| lazy(g.clone(), group)).collect()).into()
        }
    }
    let groups = |names: [&str; 2]| {
        let group = |(i, n): (usize, &&str)| Group { name: (*n).into(), items: vec![i as u32 * 10, i as u32 * 10 + 1] };
        names.iter().enumerate().map(group).collect::<Vec<Group>>()
    };
    let mut h = Harness::new(Groups(groups(["One", "Two"])));
    let w = h.window();
    let (items, outer) = (built(), GROUPS.with(Cell::get));
    assert_eq!((items, outer), (4, 2));
    h.send(());
    assert_eq!((built(), GROUPS.with(Cell::get)), (4, 2));
    // A group's name changed: the group is built, and the items it has as
    // lazy elements are found as they were.
    h.program_mut().0[1].name = "Second".into();
    h.send(());
    assert_eq!((built(), GROUPS.with(Cell::get)), (4, 3));
    // An item changed: its group and it, and not its neighbour.
    h.program_mut().0[0].items[1] = 7;
    h.send(());
    assert_eq!((built(), GROUPS.with(Cell::get)), (5, 4));
    assert_eq!(h.texts(w), ["One", "item 0", "item 7", "Second", "item 10", "item 11"]);
    let check = h.check_frame_exact(w, 1.0);
    assert_eq!(check.differing_pixels, 0, "{check:?}");
}

#[test]
fn a_lazy_element_stands_where_its_content_would() {
    aizome_kit::fonts::install();
    // What a row or a column gives a child hangs on the child's lengths
    // and on whether it's a spacer: a lazy element answers as its content.
    struct Places(bool);
    fn gap(_: &()) -> Element<()> {
        spacer().into()
    }
    fn filler(_: &()) -> Element<()> {
        container(text("fill")).fill(Tone::Chrome).width(Fill).height(Fill).into()
    }
    fn fixed(_: &()) -> Element<()> {
        container(text("fixed")).fill(Tone::Raised).width(90.0).min_height(40.0).into()
    }
    impl Program for Places {
        type Msg = ();
        fn info(&self) -> AppInfo {
            AppInfo::new("org.aizome.LazyPlaces", "Lazy Places", "Test")
        }
        fn launched(&mut self, cx: &mut Cx<()>) {
            cx.open_window(WindowSpec::new("Places").size(400.0, 200.0));
        }
        fn update(&mut self, _: (), _cx: &mut Cx<()>) {}
        fn view(&self, _w: WindowKey) -> Element<()> {
            let one = |f: fn(&()) -> Element<()>| if self.0 { lazy((), f) } else { f(&()) };
            col![
                aizome_kit::row![text("left"), one(gap), text("right").id("right")],
                aizome_kit::row![one(fixed).id("fixed"), one(filler).id("filler")].height(Fill),
                one(fixed).width(150.0).id("wider"),
            ]
            .into()
        }
    }
    let mut lazy_one = Harness::new(Places(true));
    let mut plain = Harness::new(Places(false));
    let (lw, pw) = (lazy_one.window(), plain.window());
    for name in ["right", "fixed", "filler", "wider"] {
        assert_eq!(lazy_one.find_id(lw, name), plain.find_id(pw, name), "{name}");
    }
    assert_eq!(lazy_one.find_id(lw, "wider").expect("it").width(), 150.0, "a length of its own");
    assert_eq!(lazy_one.render_rgba(lw, 1.0), plain.render_rgba(pw, 1.0));
    lazy_one.send(());
    lazy_one.resize(lw, Size::new(520.0, 260.0));
    plain.resize(pw, Size::new(520.0, 260.0));
    for name in ["right", "fixed", "filler", "wider"] {
        assert_eq!(lazy_one.find_id(lw, name), plain.find_id(pw, name), "{name}, resized");
    }
    assert_eq!(lazy_one.render_rgba(lw, 1.0), plain.render_rgba(pw, 1.0));
}

#[test]
fn lazy_rows_in_a_virtual_list_show_as_rows_built_outright() {
    aizome_kit::fonts::install();
    struct Listed {
        lazy: bool,
        ticks: u32,
    }
    fn line(i: &usize) -> Element<()> {
        aizome_kit::row![text(format!("file-{i:04}")), spacer(), text("4 KB").mono()].padding([0.0, 8.0]).into()
    }
    impl Program for Listed {
        type Msg = ();
        fn info(&self) -> AppInfo {
            AppInfo::new("org.aizome.LazyList", "Lazy List", "Test")
        }
        fn launched(&mut self, cx: &mut Cx<()>) {
            cx.open_window(WindowSpec::new("List").size(300.0, 240.0));
        }
        fn update(&mut self, _: (), _cx: &mut Cx<()>) {
            self.ticks += 1;
        }
        fn view(&self, _w: WindowKey) -> Element<()> {
            let as_lazy = self.lazy;
            let list = virtual_list(2_000, 22.0, move |i| if as_lazy { lazy(i, line) } else { line(&i) }).id("list");
            col![lazy(self.ticks, |t| text(format!("tick {t}")).into()), list].into()
        }
    }
    let mut a = Harness::new(Listed { lazy: true, ticks: 0 });
    let mut b = Harness::new(Listed { lazy: false, ticks: 0 });
    let (aw, bw) = (a.window(), b.window());
    let middle = Point::new(150.0, 120.0);
    for step in 0..6 {
        match step % 3 {
            0 => {
                a.scroll(aw, middle, Vec2::new(0.0, 130.0));
                b.scroll(bw, middle, Vec2::new(0.0, 130.0));
            }
            1 => {
                a.send(());
                b.send(());
            }
            _ => {
                a.click(aw, middle);
                b.click(bw, middle);
            }
        }
        assert_eq!(a.render_rgba(aw, 1.0), b.render_rgba(bw, 1.0), "step {step}");
        let check = a.check_frame_exact(aw, 1.0);
        assert_eq!(check.differing_pixels, 0, "step {step}: {check:?}");
    }
    // Rows come and go as the list scrolls; the window doesn't grow.
    let widgets = a.widgets(aw);
    for _ in 0..40 {
        a.scroll(aw, middle, Vec2::new(0.0, 88.0));
        a.send(());
    }
    assert_eq!(a.widgets(aw), widgets);
}

#[test]
fn many_views_leave_no_more_widgets_than_one() {
    let (mut h, w) = open(20, true, true);
    for round in 0..30u32 {
        h.send(Msg::Label(round % 20, format!("Round {round}")));
        h.send(Msg::Up((round * 7) % 20));
        h.send(Msg::Remove(round % 20));
        h.send(Msg::Add(round % 20));
        h.send(Msg::Roomy(round % 2 == 0));
    }
    h.send(Msg::Roomy(false));
    // The same rows, mounted in one go.
    let rows = h.program().rows.clone();
    let mut fresh = Harness::new(Rows { rows, ..program(0, true, true) });
    let fw = fresh.window();
    fresh.focus_window(fw, true);
    assert_eq!(h.widgets(w), fresh.widgets(fw));
    assert_eq!(h.render_rgba(w, 1.0), fresh.render_rgba(fw, 1.0));
    frame_is_whole(&mut h, w, "many views");
}

#[test]
fn a_window_takes_up_lazy_elements_and_gives_them_up_between_views() {
    // The first view with lazy elements comes to a tree that kept nothing,
    // and the first without them to one that did: each is compared with
    // what the other drew.
    let (mut h, w) = open(8, false, true);
    let _ = h.check_frame_exact(w, 1.0);
    for (round, lazy) in [true, true, false, true, false, false, true].into_iter().enumerate() {
        h.program_mut().lazy = lazy;
        h.send(Msg::Remove(round as u32));
        frame_is_whole(&mut h, w, "rows taken up or given up as lazy elements");
        h.send(Msg::Toggle(7, round % 2 == 0));
        frame_is_whole(&mut h, w, "a switch after");
    }
    assert!(h.find_id(w, "on0").is_none() && h.find_id(w, "on6").is_none());
    assert!(h.find_id(w, "on7").is_some());
    let mut fresh = Harness::new(Rows { rows: h.program().rows.clone(), ..program(0, true, true) });
    let fw = fresh.window();
    fresh.focus_window(fw, true);
    assert_eq!(h.render_rgba(w, 1.0), fresh.render_rgba(fw, 1.0));
    assert_eq!(h.widgets(w), fresh.widgets(fw));
}

#[test]
fn a_kept_element_its_parent_no_longer_lays_out_goes_as_one_built_outright_does() {
    aizome_kit::fonts::install();
    /// Shows one of its children: the rest it neither lays out nor places.
    struct Pager {
        show: usize,
        at: Point,
    }
    impl Widget for Pager {
        fn name(&self) -> &'static str {
            "pager"
        }
        fn layout(&mut self, cx: &mut LayoutCx, bc: Constraints) -> Size {
            if self.show < cx.child_count() {
                cx.layout_child(self.show, Constraints::loose(Size::new(200.0, 120.0)));
                cx.place_child(self.show, self.at);
            }
            bc.constrain(Size::new(220.0, 140.0))
        }
    }
    struct Pages {
        page: usize,
        at: Point,
        lazy: bool,
    }
    fn page(n: &usize) -> Element<()> {
        BUILT.with(|b| b.set(b.get() + 1));
        container(col![text(format!("Page {n}")).bold(), button("Go")].spacing(6.0))
            .fill(Tone::Chrome)
            .padding(16.0)
            .into()
    }
    impl Program for Pages {
        type Msg = ();
        fn info(&self) -> AppInfo {
            AppInfo::new("org.aizome.LazyPages", "Lazy Pages", "Test")
        }
        fn launched(&mut self, cx: &mut Cx<()>) {
            cx.open_window(WindowSpec::new("Pages").size(260.0, 180.0));
        }
        fn update(&mut self, _: (), _cx: &mut Cx<()>) {}
        fn view(&self, _w: WindowKey) -> Element<()> {
            let pages = (0..3).map(|n| if self.lazy { lazy(n, page) } else { page(&n) }).collect();
            Element::new(Pager { show: self.page, at: self.at }, pages)
        }
    }
    // A page placed away from the pager's corner and one placed at it: a
    // node the pager stops laying out is then where a node never placed is.
    for at in [Point::new(10.0, 10.0), Point::ZERO] {
        let mut kept = Harness::new(Pages { page: 0, at, lazy: true });
        let mut plain = Harness::new(Pages { page: 0, at, lazy: false });
        let (kw, pw) = (kept.window(), plain.window());
        let _ = kept.check_frame_exact(kw, 1.0);
        let before = built();
        for (step, show) in [1, 0, 2, 2, 1, 0, 0, 2].into_iter().enumerate() {
            kept.program_mut().page = show;
            plain.program_mut().page = show;
            kept.send(());
            let lazily = built();
            plain.send(());
            BUILT.with(|b| b.set(lazily));
            assert_eq!(kept.render_rgba(kw, 1.0), plain.render_rgba(pw, 1.0), "step {step}: page {show} at {at:?}");
            assert_eq!(kept.find(kw, "Go"), plain.find(pw, "Go"), "step {step}: the one button that shows");
            let check = kept.check_frame_exact(kw, 1.0);
            assert_eq!(check.differing_pixels, 0, "step {step}: {check:?}");
        }
        assert_eq!(built(), before, "pages were laid out again as they came back, and not built");
    }
}

#[test]
fn a_widget_in_a_kept_subtree_lives_on_and_is_drawn_as_it_is() {
    aizome_kit::fonts::install();
    /// A square that changes colour when the wheel turns over it, and says
    /// nothing of it (a press would have it drawn again anyway):
    /// it keeps that in itself, not in the window's state.
    struct Flip(bool);
    impl Widget for Flip {
        fn name(&self) -> &'static str {
            "flip"
        }
        fn layout(&mut self, _cx: &mut LayoutCx, bc: Constraints) -> Size {
            bc.constrain(Size::new(40.0, 40.0))
        }
        fn paint(&self, cx: &mut PaintCx) {
            let r = cx.bounds();
            cx.canvas.fill_rect(r, if self.0 { Color::hex(0x357d74) } else { Color::hex(0x18243a) });
        }
        fn hash(&self, h: &mut PaintHasher) {
            h.add(&self.0);
        }
        fn event(&mut self, _cx: &mut EventCx, event: &Event) -> Status {
            match event {
                Event::Scroll { .. } => {
                    self.0 = !self.0;
                    Status::Handled
                }
                _ => Status::Ignored,
            }
        }
        fn hit(&self) -> bool {
            true
        }
    }
    struct Flips(u32);
    impl Program for Flips {
        type Msg = ();
        fn info(&self) -> AppInfo {
            AppInfo::new("org.aizome.LazyFlips", "Lazy Flips", "Test")
        }
        fn launched(&mut self, cx: &mut Cx<()>) {
            cx.open_window(WindowSpec::new("Flips").size(200.0, 120.0));
        }
        fn update(&mut self, _: (), _cx: &mut Cx<()>) {
            self.0 += 1;
        }
        fn view(&self, _w: WindowKey) -> Element<()> {
            col![text(format!("tick {}", self.0)), lazy((), |_| Element::new(Flip(false), Vec::new()).id("flip"))]
                .padding(10.0)
                .into()
        }
    }
    let mut h = Harness::new(Flips(0));
    let w = h.window();
    let flip = h.find_id(w, "flip").expect("the square").center();
    let colour = |h: &mut Harness<Flips>| {
        let (width, _, px) = h.render_rgba(w, 1.0);
        let at = (flip.y as usize * width as usize + flip.x as usize) * 4;
        [px[at], px[at + 1], px[at + 2]]
    };
    let off = colour(&mut h);
    // The pointer over it, and that drawn: after this the wheel moves
    // nothing and marks nothing.
    h.move_to(w, flip);
    let _ = h.check_frame_exact(w, 1.0);
    // The wheel: it draws differently, and is drawn, though it asked for
    // nothing and no view came.
    h.scroll(w, flip, Vec2::new(0.0, 10.0));
    let on = colour(&mut h);
    assert_ne!(on, off);
    let check = h.check_frame_exact(w, 1.0);
    assert_eq!(check.differing_pixels, 0, "{check:?}");
    // Views come and go: the widget is the same one, as it was left.
    h.send(());
    h.send(());
    assert_eq!(colour(&mut h), on);
    let check = h.check_frame_exact(w, 1.0);
    assert_eq!(check.differing_pixels, 0, "{check:?}");
    h.scroll(w, flip, Vec2::new(0.0, 10.0));
    assert_eq!(colour(&mut h), off);
    let check = h.check_frame_exact(w, 1.0);
    assert_eq!(check.differing_pixels, 0, "{check:?}");
}

mod coming_and_going {
    //! Rows that leave a window and come back, as a list with part of it
    //! folded away does: the tree keeps its nodes from view to view, so
    //! what it does with the ones that died matters.

    use super::*;

    thread_local! {
        /// Marks dropped on this thread.
        static DROPPED: Cell<usize> = const { Cell::new(0) };
    }

    fn dropped() -> usize {
        DROPPED.with(Cell::get)
    }

    /// A square that counts when it's dropped: each row has one.
    struct Mark;

    impl Drop for Mark {
        fn drop(&mut self) {
            DROPPED.with(|d| d.set(d.get() + 1));
        }
    }

    impl Widget for Mark {
        fn name(&self) -> &'static str {
            "mark"
        }

        fn layout(&mut self, _cx: &mut LayoutCx, bc: Constraints) -> Size {
            bc.constrain(Size::new(8.0, 8.0))
        }
    }

    /// Nodes to a row: the lazy element, its row, a label and a mark.
    const TO_A_ROW: usize = 4;

    struct Folding {
        rows: usize,
        /// The rows that don't show.
        folded: std::ops::Range<usize>,
    }

    fn folding_row(n: &usize) -> Element<()> {
        aizome_kit::row![text(format!("row {n}")), Element::new(Mark, Vec::new())].spacing(4.0).into()
    }

    impl Program for Folding {
        type Msg = ();
        fn info(&self) -> AppInfo {
            AppInfo::new("org.aizome.LazyFolding", "Lazy Folding", "Test")
        }
        fn launched(&mut self, cx: &mut Cx<()>) {
            cx.open_window(WindowSpec::new("Folding").size(240.0, 300.0));
        }
        fn update(&mut self, _: (), _cx: &mut Cx<()>) {}
        fn view(&self, _w: WindowKey) -> Element<()> {
            let rows = (0..self.rows).filter(|n| !self.folded.contains(n)).map(|n| lazy(n, folding_row).key(n));
            scroll(column(rows.collect())).into()
        }
    }

    fn open(rows: usize) -> (Harness<Folding>, WindowKey) {
        aizome_kit::fonts::install();
        let h = Harness::new(Folding { rows, folded: 0..0 });
        let w = h.window();
        (h, w)
    }

    fn fold(h: &mut Harness<Folding>, folded: std::ops::Range<usize>) {
        h.program_mut().folded = folded;
        h.send(());
    }

    #[test]
    fn rows_folded_away_at_the_end_and_shown_again_leave_nothing_behind() {
        let (mut h, w) = open(600);
        let all = h.widgets(w);
        assert_eq!(h.nodes_held(w), all);
        // A tenth of the rows, the last, away and back by turns: what a
        // structural update in a long window is.
        for turn in 0..40 {
            let before = dropped();
            fold(&mut h, 540..600);
            assert_eq!(h.widgets(w), all - 60 * TO_A_ROW, "turn {turn}");
            assert_eq!(h.nodes_held(w), h.widgets(w), "turn {turn}: the dead ones' room was given up");
            assert_eq!(dropped(), before + 60, "turn {turn}: and their widgets with them");
            fold(&mut h, 0..0);
            assert_eq!(h.widgets(w), all, "turn {turn}");
            assert_eq!(h.nodes_held(w), all, "turn {turn}: shown again, in the room they left");
        }
        let check = h.check_frame_exact(w, 1.0);
        assert_eq!(check.differing_pixels, 0, "{check:?}");
    }

    #[test]
    fn rows_folded_away_in_the_middle_give_up_their_widgets_at_once() {
        let (mut h, w) = open(600);
        let all = h.widgets(w);
        // The same sixty in the middle, away and back: their first nodes'
        // room waits among the others' for the dead to be dropped together,
        // and no more room than that is ever held.
        for turn in 0..40 {
            let before = dropped();
            fold(&mut h, 200..260);
            assert_eq!(dropped(), before + 60, "turn {turn}: a dead row's widgets go when it dies");
            assert!(h.nodes_held(w) <= all, "turn {turn}: {} held for {all}", h.nodes_held(w));
            fold(&mut h, 0..0);
            assert_eq!(h.widgets(w), all);
            assert!(h.nodes_held(w) <= all + 60 * TO_A_ROW, "turn {turn}: {} held for {all}", h.nodes_held(w));
        }
    }

    #[test]
    fn rows_folded_away_here_and_there_are_dropped_before_they_outnumber_the_living() {
        let (mut h, w) = open(600);
        let all = h.widgets(w);
        // Sixty away, a different sixty each time, the ones before shown
        // again at the end of the tree's nodes: dead nodes gather among the
        // living until they are dropped together.
        let mut most = 0;
        let mut away = 0..0;
        for turn in 0..200 {
            let from = (turn * 37) % 540;
            let next = from..from + 60;
            let going = next.clone().filter(|n| !away.contains(n)).count();
            let before = dropped();
            fold(&mut h, next.clone());
            away = next;
            assert_eq!(dropped(), before + going, "turn {turn}: the rows that went gave up their widgets");
            most = most.max(h.nodes_held(w));
            assert!(h.nodes_held(w) <= 2 * all + 256, "turn {turn}: {} held for {all}", h.nodes_held(w));
        }
        fold(&mut h, 0..0);
        assert_eq!(h.widgets(w), all);
        assert!(most > all, "dead nodes did wait among the living ({most} held at most)");
        let check = h.check_frame_exact(w, 1.0);
        assert_eq!(check.differing_pixels, 0, "{check:?}");
    }
}
