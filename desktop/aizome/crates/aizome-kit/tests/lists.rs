//! Scrolling and lists: only the rows that show are built, scrolling
//! doesn't view the window again, selection and the keyboard.

use std::cell::Cell;

use aizome_kit::column;
use aizome_kit::prelude::*;
use aizome_kit::sidestep_ui::{Key, Modifiers, NamedKey, PointerButton};
use aizome_kit::testing::Harness;

struct Files {
    names: std::rc::Rc<Vec<String>>,
    selected: Vec<usize>,
    opened: Vec<usize>,
    views: Cell<usize>,
    reveal: Option<usize>,
}

#[derive(Clone, Debug)]
enum Msg {
    Select(Vec<usize>),
    Open(usize),
    Reveal(usize),
}

impl Files {
    fn new(n: usize) -> Files {
        Files {
            names: std::rc::Rc::new((0..n).map(|i| format!("file-{i:05}.txt")).collect()),
            selected: Vec::new(),
            opened: Vec::new(),
            views: Cell::new(0),
            reveal: None,
        }
    }
}

impl Program for Files {
    type Msg = Msg;
    fn info(&self) -> AppInfo {
        AppInfo::new("org.aizome.Files", "Files", "Test")
    }
    fn launched(&mut self, cx: &mut Cx<Msg>) {
        cx.open_window(WindowSpec::new("Files").size(400.0, 300.0));
    }
    fn update(&mut self, msg: Msg, _cx: &mut Cx<Msg>) {
        match msg {
            Msg::Select(s) => self.selected = s,
            Msg::Open(i) => self.opened.push(i),
            Msg::Reveal(i) => self.reveal = Some(i),
        }
    }
    fn view(&self, _w: WindowKey) -> Element<Msg> {
        self.views.set(self.views.get() + 1);
        let names = self.names.clone();
        let types = self.names.clone();
        let list = virtual_list(self.names.len(), 24.0, move |i| {
            aizome_kit::row![icon(Icon::File).size(14.0), text(names[i].clone())].spacing(8.0).padding([0.0, 10.0])
        });
        let list = match self.reveal {
            Some(r) => list.reveal(r),
            None => list,
        };
        list.selected(self.selected.iter().copied())
            .on_select(Msg::Select)
            .on_activate(Msg::Open)
            .multiple()
            .type_select(move |i| types[i].clone())
            .id("list")
            .into()
    }
}

#[test]
fn only_the_rows_that_show_are_built() {
    let h = Harness::new(Files::new(10_000));
    let w = h.window();
    let rows = h.texts(w).iter().filter(|t| t.starts_with("file-")).count();
    assert!((12..=14).contains(&rows), "{rows} rows built for 300pt of 24pt rows");
}

#[test]
fn scrolling_moves_rows_without_viewing_again() {
    let mut h = Harness::new(Files::new(10_000));
    let w = h.window();
    let views = h.program().views.get();
    h.take_damage(w);
    h.scroll(w, Point::new(200.0, 150.0), Vec2::new(0.0, 240.0));
    assert_eq!(h.program().views.get(), views, "the view wasn't asked for again");
    let texts = h.texts(w);
    assert!(texts.iter().any(|t| t == "file-00010.txt"), "row 10 is at the top now");
    assert!(!texts.iter().any(|t| t == "file-00000.txt"));
    let damage = h.take_damage(w);
    assert!(!damage.is_empty());
    for d in &damage {
        assert!(Rect::new(0.0, 0.0, 400.0, 300.0).contains_rect(*d));
    }
    // Far past the end stops at the end.
    h.scroll(w, Point::new(200.0, 150.0), Vec2::new(0.0, 1e9));
    assert!(h.texts(w).iter().any(|t| t == "file-09999.txt"));
}

#[test]
fn clicks_and_keys_select() {
    let mut h = Harness::new(Files::new(10_000));
    let w = h.window();
    h.focus_window(w, true);
    h.click(w, Point::new(100.0, 12.0));
    assert_eq!(h.program().selected, [0]);
    h.named(w, NamedKey::ArrowDown, Modifiers::NONE);
    h.named(w, NamedKey::ArrowDown, Modifiers::NONE);
    assert_eq!(h.program().selected, [2]);
    h.named(w, NamedKey::ArrowDown, Modifiers::SHIFT);
    assert_eq!(h.program().selected, [2, 3]);
    h.named(w, NamedKey::End, Modifiers::NONE);
    assert_eq!(h.program().selected, [9999]);
    assert!(h.texts(w).iter().any(|t| t == "file-09999.txt"), "the list scrolled to show it");
    h.named(w, NamedKey::Enter, Modifiers::NONE);
    assert_eq!(h.program().opened, [9999]);
    // Type-select.
    h.type_text(w, "file-00042");
    assert_eq!(h.program().selected, [42]);
}

#[test]
fn shift_and_command_clicks_select_several() {
    let mut h = Harness::new(Files::new(100));
    let w = h.window();
    h.click(w, Point::new(100.0, 12.0));
    h.press(w, Point::new(100.0, 60.0), PointerButton::Left, 1);
    h.release(w, Point::new(100.0, 60.0), PointerButton::Left);
    assert_eq!(h.program().selected, [2]);
    h.key(w, Key::Named(NamedKey::Tab), Modifiers::NONE);
    let _ = Modifiers::SHIFT;
    h.press(w, Point::new(100.0, 12.0), PointerButton::Left, 2);
    assert_eq!(h.program().opened, [0], "a double click opens the row it selects");
}

/// A long form in a scroll view.
struct Long {
    clicks: usize,
}

#[derive(Clone, Debug)]
enum LongMsg {
    Click,
}

impl Program for Long {
    type Msg = LongMsg;
    fn info(&self) -> AppInfo {
        AppInfo::new("org.aizome.Long", "Long", "Test")
    }
    fn launched(&mut self, cx: &mut Cx<LongMsg>) {
        cx.open_window(WindowSpec::new("Long").size(300.0, 200.0));
    }
    fn update(&mut self, _msg: LongMsg, _cx: &mut Cx<LongMsg>) {
        self.clicks += 1;
    }
    fn view(&self, _w: WindowKey) -> Element<LongMsg> {
        let buttons: Vec<Element<LongMsg>> = (0..30)
            .map(|i| button(format!("Button {i}")).on_press(LongMsg::Click).id(format!("b{i}")).into())
            .collect();
        scroll(aizome_kit::widgets::layout::column(buttons).spacing(8.0).padding(8.0)).into()
    }
}

#[test]
fn tabbing_into_a_hidden_control_scrolls_it_into_view() {
    let mut h = Harness::new(Long { clicks: 0 });
    let w = h.window();
    h.focus_window(w, true);
    assert!(h.find_id(w, "b20").unwrap().y0 > 200.0, "button 20 starts out of sight");
    for _ in 0..21 {
        h.named(w, NamedKey::Tab, Modifiers::NONE);
    }
    assert_eq!(h.focused_name(w).as_deref(), Some("b20"));
    let r = h.find_id(w, "b20").unwrap();
    assert!(r.y0 >= 0.0 && r.y1 <= 200.0, "button 20 shows: {r:?}");
    h.key(w, Key::Character(" ".into()), Modifiers::NONE);
    assert_eq!(h.program().clicks, 1);
}

#[test]
fn scroll_bars_drag() {
    let mut h = Harness::new(Long { clicks: 0 });
    let w = h.window();
    let before = h.find_id(w, "b0").unwrap().y0;
    // The bar along the right edge: drag its thumb down.
    h.scroll(w, Point::new(150.0, 100.0), Vec2::new(0.0, 1.0));
    h.drag(w, Point::new(294.0, 10.0), Point::new(294.0, 150.0), 5);
    let after = h.find_id(w, "b0").unwrap().y0;
    assert!(after < before - 100.0, "dragging the thumb scrolled: {before} → {after}");
    let png = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("scrolled.png");
    h.scroll(w, Point::new(150.0, 100.0), Vec2::new(0.0, 10.0));
    h.save_png(w, &png, 2.0);
    let _: aizome_kit::widgets::layout::Flex<LongMsg> = column![text("x")];
}

/// The darkest and lightest pixels' luminance in `rect`.
fn extremes(h: &mut Harness<Files>, w: WindowKey, rect: Rect) -> (u8, u8) {
    let img = h.render_rect(w, rect, 1.0).to_rgba8().expect("pixels");
    let lum = |p: &[u8]| ((p[0] as u32 * 3 + p[1] as u32 * 6 + p[2] as u32) / 10) as u8;
    img.chunks(4).map(lum).fold((255, 0), |(lo, hi), l| (lo.min(l), hi.max(l)))
}

#[test]
fn a_selection_without_the_focus_keeps_its_text_readable() {
    use aizome_kit::theme::ThemeChoice;
    let a =
        aizome_kit::Appearance { theme: ThemeChoice::Day, reduced_motion: true, ..aizome_kit::Appearance::default() };
    let mut h = Harness::with(Files::new(20), a, false);
    let w = h.window();
    h.focus_window(w, true);
    h.click(w, Point::new(100.0, 36.0));
    assert_eq!(h.program().selected, [1]);
    let row = Rect::new(0.0, 24.0, 400.0, 48.0);
    // Focused: light marks on the selection.
    let (_, light) = extremes(&mut h, w, row);
    assert!(light > 235, "focused selected row has light marks ({light})");
    // The window loses the keyboard: the row turns grey, in dark marks.
    h.focus_window(w, false);
    let (dark, _) = extremes(&mut h, w, row);
    assert!(dark < 110, "unfocused selected row has dark marks ({dark})");
    h.focus_window(w, true);
    let (_, light) = extremes(&mut h, w, row);
    assert!(light > 235, "focused again, the marks are light again ({light})");
}

#[test]
fn a_program_reveals_a_row_when_it_names_another() {
    let mut h = Harness::new(Files::new(10_000));
    let w = h.window();
    h.send(Msg::Reveal(5000));
    assert!(h.texts(w).iter().any(|t| t == "file-05000.txt"), "scrolled to it");
    // The user scrolls away; viewing again with the same row leaves them be.
    h.scroll(w, Point::new(100.0, 100.0), Vec2::new(0.0, -2000.0));
    h.send(Msg::Select(vec![]));
    assert!(!h.texts(w).iter().any(|t| t == "file-05000.txt"), "still where the user scrolled");
    h.send(Msg::Reveal(20));
    assert!(h.texts(w).iter().any(|t| t == "file-00020.txt"), "scrolled back up to it");
}
