//! Scrolling moves what's drawn as pixels and draws only what it uncovers:
//! each frame, composed as a compositor's buffer gets it (the last frame
//! moved, the damage drawn), must be the window drawn whole.

use std::sync::Arc;

use aizome_kit::column;
use aizome_kit::prelude::*;
use aizome_kit::sidestep_ui::{Modifiers, NamedKey};
use aizome_kit::testing::{FrameCheck, Harness};

#[derive(Clone, Copy, Debug, PartialEq)]
enum Page {
    List,
    Scroll,
    Table,
    Tree,
    Editor,
    Covered,
}

struct Pages {
    page: Page,
    selected: Vec<usize>,
    content: EditorContent,
    open: Arc<std::collections::HashSet<u64>>,
}

#[derive(Clone, Debug)]
enum Msg {
    Select(Vec<usize>),
    Edit(EditorAction),
    Pressed,
}

impl Pages {
    fn new(page: Page) -> Pages {
        let text: String = (0..400)
            .map(|i| format!("Line {i}: operations stay in points, each window at its output's scale.\n"))
            .collect();
        Pages {
            page,
            selected: vec![3],
            content: EditorContent::new(&text),
            open: Arc::new((0..40u64).map(|i| i * 100).collect()),
        }
    }
}

impl Program for Pages {
    type Msg = Msg;
    fn info(&self) -> AppInfo {
        AppInfo::new("org.aizome.Scrolling", "Scrolling", "Test")
    }
    fn launched(&mut self, cx: &mut Cx<Msg>) {
        cx.open_window(WindowSpec::new("Scrolling").size(400.0, 300.0));
    }
    fn update(&mut self, msg: Msg, _cx: &mut Cx<Msg>) {
        match msg {
            Msg::Select(s) => self.selected = s,
            Msg::Edit(a) => {
                self.content.perform(a);
            }
            Msg::Pressed => {}
        }
    }
    fn view(&self, _w: WindowKey) -> Element<Msg> {
        let rows = |i: usize| {
            aizome_kit::row![
                icon(Icon::FileText).size(14.0),
                text(format!("file-{i:05}.md")),
                spacer(),
                text(format!("{} KB", i * 7)).mono()
            ]
            .spacing(8.0)
            .padding([0.0, 10.0])
        };
        match self.page {
            Page::List => virtual_list(10_000, 24.0, rows)
                .selected(self.selected.iter().copied())
                .on_select(Msg::Select)
                .id("view")
                .into(),
            Page::Scroll => scroll(
                column(
                    (0..80)
                        .map(|i| {
                            if i % 3 == 0 {
                                button(format!("Button {i}")).on_press(Msg::Pressed).id(format!("b{i}")).into()
                            } else {
                                text(format!("A line of text, number {i}, long enough to wrap at this width maybe."))
                                    .wrap()
                                    .into()
                            }
                        })
                        .collect::<Vec<_>>(),
                )
                .spacing(6.0)
                .padding(12.0),
            )
            .id("view")
            .into(),
            Page::Table => table(
                vec![
                    Column::new("Name").width(220.0),
                    Column::new("Kind").width(160.0),
                    Column::new("Size").width(120.0).align_end(),
                ],
                5_000,
                |r, c| -> Element<Msg> {
                    match c {
                        0 => text(format!("file-{r:05}.md")).into(),
                        1 => text("Markdown document").role(Role::Text2).into(),
                        _ => text(format!("{} KB", r * 13 % 9000)).mono().into(),
                    }
                },
            )
            .selected(self.selected.iter().copied())
            .on_select(Msg::Select)
            .id("view")
            .into(),
            Page::Tree => {
                let items: Vec<TreeItem> = (0..40u64)
                    .map(|b| {
                        TreeItem::branch(
                            b * 100,
                            format!("folder {b}"),
                            (1..8)
                                .map(|l| TreeItem::leaf(b * 100 + l, format!("file {l}")).icon(Icon::FileText))
                                .collect(),
                        )
                        .icon(Icon::Folder)
                    })
                    .collect();
                tree(items).expanded(self.open.clone()).id("view").into()
            }
            Page::Editor => {
                text_editor(&self.content).on_action(Msg::Edit).line_numbers(true).wrap(false).id("view").into()
            }
            Page::Covered => stack![
                virtual_list(10_000, 24.0, rows).id("view"),
                container(text("Over the list")).padding(8.0).fill(Tone::Raised).border(Edge::Line).center(),
            ]
            .into(),
        }
    }
}

fn harness(page: Page) -> (Harness<Pages>, WindowKey) {
    let mut h = Harness::new(Pages::new(page));
    let w = h.window();
    h.focus_window(w, true);
    // The first frame: drawn whole.
    h.check_frame(w);
    (h, w)
}

fn scroll_by(h: &mut Harness<Pages>, w: WindowKey, delta: Vec2) -> FrameCheck {
    let at = h.find_id(w, "view").unwrap().center();
    h.scroll(w, at, delta);
    h.check_frame(w)
}

#[track_caller]
fn moved(c: FrameCheck, area: f64) {
    assert_eq!(c.differs, 0.0, "the frame composed differs from the window drawn whole: {c:?}");
    assert!(c.moves > 0, "pixels moved: {c:?}");
    assert!(c.damaged < area, "drawn: {c:?}, more than {area} pt²");
}

const WINDOW: f64 = 400.0 * 300.0;

#[test]
fn a_list_moves_its_rows_and_draws_the_strip() {
    let (mut h, w) = harness(Page::List);
    for d in [40.0, 40.0, 13.0, -27.0, 200.0, -80.0] {
        moved(scroll_by(&mut h, w, Vec2::new(0.0, d)), 400.0 * (f64::abs(d) + 40.0));
    }
    // A selection changing as it scrolls.
    h.click(w, Point::new(100.0, 100.0));
    h.check_frame(w);
    h.named(w, NamedKey::ArrowDown, Modifiers::NONE);
    // (What waited to be drawn is drawn where it was and where it went.)
    let c = scroll_by(&mut h, w, Vec2::new(0.0, 48.0));
    moved(c, WINDOW * 0.6);
}

#[test]
fn a_scroll_view_moves_its_content() {
    let (mut h, w) = harness(Page::Scroll);
    for d in [40.0, 41.0, -15.0, 120.0] {
        moved(scroll_by(&mut h, w, Vec2::new(0.0, d)), 400.0 * (f64::abs(d) + 40.0));
    }
    // A focused button (its ring reaches out of it) scrolled.
    h.named(w, NamedKey::Tab, Modifiers::NONE);
    h.check_frame(w);
    moved(scroll_by(&mut h, w, Vec2::new(0.0, -40.0)), WINDOW * 0.5);
}

#[test]
fn a_table_moves_its_rows_and_its_header_sideways() {
    let (mut h, w) = harness(Page::Table);
    moved(scroll_by(&mut h, w, Vec2::new(0.0, 40.0)), WINDOW * 0.25);
    moved(scroll_by(&mut h, w, Vec2::new(0.0, 24.0)), WINDOW * 0.25);
    moved(scroll_by(&mut h, w, Vec2::new(30.0, 0.0)), WINDOW * 0.25);
    moved(scroll_by(&mut h, w, Vec2::new(-10.0, 40.0)), WINDOW * 0.4);
}

#[test]
fn an_outline_moves_its_rows() {
    let (mut h, w) = harness(Page::Tree);
    for d in [40.0, 48.0, -24.0] {
        moved(scroll_by(&mut h, w, Vec2::new(0.0, d)), WINDOW * 0.3);
    }
}

#[test]
fn an_editor_moves_its_lines() {
    let (mut h, w) = harness(Page::Editor);
    for d in [40.0, 63.0, -21.0] {
        moved(scroll_by(&mut h, w, Vec2::new(0.0, d)), WINDOW * 0.35);
    }
    // Sideways, the text moves and the gutter stays.
    moved(scroll_by(&mut h, w, Vec2::new(30.0, 0.0)), WINDOW * 0.3);
    // The caret going down a page scrolls it too.
    let e = h.find_id(w, "view").unwrap();
    h.click(w, Point::new(e.x0 + 120.0, e.y0 + 40.0));
    h.check_frame(w);
    for _ in 0..14 {
        h.named(w, NamedKey::ArrowDown, Modifiers::NONE);
        let c = h.check_frame(w);
        assert_eq!(c.differs, 0.0, "{c:?}");
    }
}

#[test]
fn something_drawn_over_a_list_has_it_drawn_whole() {
    let (mut h, w) = harness(Page::Covered);
    let c = scroll_by(&mut h, w, Vec2::new(0.0, 40.0));
    assert_eq!(c.differs, 0.0, "{c:?}");
    assert_eq!(c.moves, 0, "nothing moved under what's over it: {c:?}");
}
