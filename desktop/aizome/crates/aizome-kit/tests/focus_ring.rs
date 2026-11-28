//! The focus ring is drawn over everything, around the focused widget, and
//! a little past what clips the widget (a field flush with its scroll
//! view's edge shows its ring whole). So part of it lies outside the
//! widget's own drawing and outside the pixels a scroll moves: wherever
//! the ring goes, and whenever it comes or goes, each frame made of the
//! damage must still be the window drawn whole.

use aizome_kit::prelude::*;
use aizome_kit::testing::Harness;
use aizome_kit::widgets::layout::column;

struct Fields {
    notes: Vec<String>,
    /// A banner above the fields, which moves them all down.
    banner: bool,
    /// Room above the first field, inside the scroll view: the fields move
    /// down in it, and it stays where it is.
    room: bool,
}

#[derive(Clone, Debug)]
enum Msg {
    Note(usize, String),
    Banner(bool),
    Room(bool),
}

impl Program for Fields {
    type Msg = Msg;

    fn info(&self) -> AppInfo {
        AppInfo::new("org.aizome.FocusRing", "Focus Ring", "Test")
    }

    fn launched(&mut self, cx: &mut Cx<Msg>) {
        cx.open_window(WindowSpec::new("Fields").size(420.0, 300.0));
    }

    fn update(&mut self, msg: Msg, _cx: &mut Cx<Msg>) {
        match msg {
            Msg::Note(i, s) => self.notes[i] = s,
            Msg::Banner(on) => self.banner = on,
            Msg::Room(on) => self.room = on,
        }
    }

    fn view(&self, _w: WindowKey) -> Element<Msg> {
        let rows: Vec<Element<Msg>> = self
            .notes
            .iter()
            .enumerate()
            .map(|(i, note)| {
                aizome_kit::row![
                    text(format!("Field {i}")).width(90.0),
                    text_field(note).on_change(move |s| Msg::Note(i, s)).id(format!("field{i}")).width(140.0),
                ]
                .spacing(8.0)
                .padding([2.0, 8.0])
                .key(i)
                .into()
            })
            .collect();
        let mut all: Vec<Element<Msg>> = Vec::new();
        if self.banner {
            all.push(text("A banner over the fields").into());
        }
        all.push(text_field("").id("outside").width(140.0).into());
        let top = if self.room { 12.0 } else { 0.0 };
        all.push(scroll(column(rows).padding([top, 0.0, 0.0, 0.0])).id("fields").into());
        column(all).spacing(6.0).padding(10.0).into()
    }
}

fn open() -> (Harness<Fields>, WindowKey) {
    aizome_kit::fonts::install();
    let mut h = Harness::new(Fields { notes: vec![String::new(); 30], banner: false, room: false });
    let w = h.window();
    h.focus_window(w, true);
    let _ = h.check_frame_exact(w, 1.0);
    (h, w)
}

#[track_caller]
fn frame_is_whole(h: &mut Harness<Fields>, w: WindowKey, after: &str) {
    let check = h.check_frame_exact(w, 1.0);
    assert_eq!(check.differing_pixels, 0, "after {after}: {check:?}");
}

/// Scroll the fields so `name`'s top is `below` points under the scroll
/// view's top edge (above it, if negative).
fn bring_to_edge(h: &mut Harness<Fields>, w: WindowKey, name: &str, below: f64) {
    let view = h.find_id(w, "fields").expect("the scroll view");
    let field = h.find_id(w, name).expect("the field");
    h.scroll(w, view.center(), Vec2::new(0.0, field.y0 - view.y0 - below));
    let now = h.find_id(w, name).expect("the field");
    assert_eq!(now.y0, view.y0 + below, "the field where it was asked to be");
}

#[test]
fn a_ring_scrolled_across_its_scroll_view_s_edge_leaves_nothing_behind() {
    let (mut h, w) = open();
    h.click(w, h.find_id(w, "field3").expect("a field").center());
    assert_eq!(h.focused_name(w).as_deref(), Some("field3"));
    frame_is_whole(&mut h, w, "the focus");
    // Up to the edge, across it a point at a time (the ring is two points
    // wide, outside the field, and shows three points past the edge), and
    // out of sight; then back.
    bring_to_edge(&mut h, w, "field3", 6.0);
    frame_is_whole(&mut h, w, "the field near the edge");
    let middle = h.find_id(w, "fields").expect("the scroll view").center();
    for step in 0..40 {
        h.scroll(w, middle, Vec2::new(0.0, 1.0));
        frame_is_whole(&mut h, w, &format!("a point up, {step} times"));
    }
    for step in 0..40 {
        h.scroll(w, middle, Vec2::new(0.0, -1.0));
        frame_is_whole(&mut h, w, &format!("a point back, {step} times"));
    }
    // In one turn of the wheel, as the wheel goes.
    h.scroll(w, middle, Vec2::new(0.0, 60.0));
    frame_is_whole(&mut h, w, "a turn of the wheel");
    h.scroll(w, middle, Vec2::new(0.0, -25.0));
    frame_is_whole(&mut h, w, "part of a turn back");
}

#[test]
fn a_ring_past_the_edge_comes_and_goes_with_the_focus_and_the_keyboard() {
    let (mut h, w) = open();
    h.click(w, h.find_id(w, "field3").expect("a field").center());
    // The field's top a point under the edge: its ring lies across it.
    bring_to_edge(&mut h, w, "field3", 1.0);
    frame_is_whole(&mut h, w, "the ring across the edge");
    // The window loses the keyboard and gets it back.
    h.focus_window(w, false);
    frame_is_whole(&mut h, w, "the keyboard gone");
    h.focus_window(w, true);
    frame_is_whole(&mut h, w, "the keyboard back");
    // The focus goes to a field outside the scroll view, and comes back
    // by a click on what shows of the field.
    h.click(w, h.find_id(w, "outside").expect("the other field").center());
    assert_eq!(h.focused_name(w).as_deref(), Some("outside"));
    frame_is_whole(&mut h, w, "the focus elsewhere");
    let field = h.find_id(w, "field3").expect("the field");
    h.click(w, Point::new(field.center().x, field.y1 - 3.0));
    assert_eq!(h.focused_name(w).as_deref(), Some("field3"));
    frame_is_whole(&mut h, w, "the focus back");
}

#[test]
fn a_ring_past_the_edge_moves_with_its_scroll_view() {
    let (mut h, w) = open();
    h.click(w, h.find_id(w, "field3").expect("a field").center());
    bring_to_edge(&mut h, w, "field3", 1.0);
    frame_is_whole(&mut h, w, "the ring across the edge");
    // A view puts something above the scroll view: the fields move down
    // with it, ring and all, and no pixels are moved for it.
    h.send(Msg::Banner(true));
    frame_is_whole(&mut h, w, "a banner above");
    h.send(Msg::Note(3, "typed".into()));
    frame_is_whole(&mut h, w, "the field changed");
    h.send(Msg::Banner(false));
    frame_is_whole(&mut h, w, "the banner gone");
    // And the window resized, which draws it whole and must leave the
    // ring's place known for what follows.
    h.resize(w, Size::new(420.0, 260.0));
    let _ = h.check_frame_exact(w, 1.0);
    let middle = h.find_id(w, "fields").expect("the scroll view").center();
    h.scroll(w, middle, Vec2::new(0.0, 2.0));
    frame_is_whole(&mut h, w, "a scroll after a resize");
}

#[test]
fn a_ring_past_the_edge_is_followed_when_a_view_moves_its_field() {
    let (mut h, w) = open();
    h.click(w, h.find_id(w, "field3").expect("a field").center());
    bring_to_edge(&mut h, w, "field3", 1.0);
    frame_is_whole(&mut h, w, "the ring across the edge");
    // A view moves the fields down inside the scroll view, which stays
    // where it is and scrolls nothing: the ring leaves the edge with its
    // field, and comes back to it.
    let before = h.find_id(w, "field3").expect("the field");
    h.send(Msg::Room(true));
    assert_eq!(h.find_id(w, "field3").expect("the field").y0, before.y0 + 12.0);
    frame_is_whole(&mut h, w, "the field moved off the edge by a view");
    h.send(Msg::Room(false));
    assert_eq!(h.find_id(w, "field3").expect("the field").y0, before.y0);
    frame_is_whole(&mut h, w, "the field moved back onto it");
}
