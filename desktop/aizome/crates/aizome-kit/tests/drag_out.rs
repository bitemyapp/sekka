//! Dragging out of the windows: from a mouse area, a list's rows, a
//! table's; the program asked for promised data and told how it ended.

use std::cell::RefCell;
use std::rc::Rc;

use aizome_kit::column;
use aizome_kit::prelude::*;
use aizome_kit::sidestep_ui::PointerButton;
use aizome_kit::testing::Harness;

#[derive(Default)]
struct Files {
    selected: Vec<usize>,
    dragged: Rc<RefCell<Vec<Vec<usize>>>>,
    drags: Vec<DragEvent>,
    ended: Vec<(DragId, DropAction)>,
    table: bool,
}

#[derive(Clone, Debug)]
enum Msg {
    Select(Vec<usize>),
    Drag(DragEvent),
}

fn source(rows: &[usize]) -> DragSource {
    DragSource::new().urls(rows.iter().map(|r| format!("file:///home/u/file-{r}.md"))).actions(true, false)
}

impl Program for Files {
    type Msg = Msg;
    fn info(&self) -> AppInfo {
        AppInfo::new("org.aizome.DragOut", "DragOut", "Test")
    }
    fn launched(&mut self, cx: &mut Cx<Msg>) {
        cx.open_window(WindowSpec::new("DragOut").size(400.0, 300.0));
    }
    fn update(&mut self, msg: Msg, _cx: &mut Cx<Msg>) {
        match msg {
            Msg::Select(s) => self.selected = s,
            Msg::Drag(d) => self.drags.push(d),
        }
    }
    fn view(&self, _w: WindowKey) -> Element<Msg> {
        let dragged = self.dragged.clone();
        let rows: Element<Msg> = if self.table {
            table(vec![Column::new("Name").width(200.0)], 100, |r, _| text(format!("file-{r}.md")))
                .selected(self.selected.iter().copied())
                .on_select(Msg::Select)
                .multiple()
                .drag_out(move |rows| {
                    dragged.borrow_mut().push(rows.to_vec());
                    source(rows)
                })
                .id("rows")
                .into()
        } else {
            virtual_list(100, 24.0, |i| text(format!("file-{i}.md")))
                .selected(self.selected.iter().copied())
                .on_select(Msg::Select)
                .multiple()
                .drag_out(move |rows| {
                    dragged.borrow_mut().push(rows.to_vec());
                    source(rows)
                })
                .id("rows")
                .into()
        };
        column![
            mouse_area(container(text("Drag me")).padding(10.0))
                .on_drag(Msg::Drag)
                .drag_out(|| DragSource::new().text("dragged text").promise("image/png"))
                .id("chip"),
            container(rows).height(Fill),
        ]
        .into()
    }
    fn provide_drag_data(&mut self, _drag: DragId, mime: &str, _cx: &mut Cx<Msg>) -> Option<Vec<u8>> {
        (mime == "image/png").then(|| b"PNG".to_vec())
    }
    fn drag_ended(&mut self, drag: DragId, action: DropAction, _cx: &mut Cx<Msg>) {
        self.ended.push((drag, action));
    }
}

fn press_and_move(h: &mut Harness<Files>, w: WindowKey, from: Point, by: Vec2) {
    h.press(w, from, PointerButton::Left, 1);
    h.move_to(w, from + by * 0.25);
    h.move_to(w, from + by);
}

#[test]
fn a_mouse_area_drags_out_past_a_few_points() {
    let mut h = Harness::new(Files::default());
    let w = h.window();
    let chip = h.find_id(w, "chip").unwrap().center();
    h.press(w, chip, PointerButton::Left, 1);
    h.move_to(w, chip + Vec2::new(2.0, 0.0));
    assert!(h.drags().is_empty(), "not yet: two points");
    h.move_to(w, chip + Vec2::new(12.0, 3.0));
    h.move_to(w, chip + Vec2::new(40.0, 3.0));
    let drags = h.drags();
    assert_eq!(drags.len(), 1);
    let (id, carried) = drags[0];
    assert!(
        format!("{carried:?}").contains("text/plain") && format!("{carried:?}").contains("image/png"),
        "{carried:?}"
    );
    // on_drag heard the start and the moves up to the drag, then nothing.
    let moves = h.program().drags.iter().filter(|d| matches!(d, DragEvent::Move { .. })).count();
    assert!(matches!(h.program().drags[0], DragEvent::Start(_)) && moves == 1, "{:?}", h.program().drags);
    // The data promised is the program's to make; then it ends.
    assert_eq!(h.drag_data(id, "image/png").as_deref(), Some(&b"PNG"[..]));
    assert_eq!(h.drag_data(id, "text/html"), None);
    h.end_drag(id, DropAction::Copy);
    assert_eq!(h.program().ended, [(id, DropAction::Copy)]);
}

#[test]
fn list_rows_drag_out_as_the_selection_or_the_row() {
    for table in [false, true] {
        let mut h = Harness::new(Files { table, ..Files::default() });
        let w = h.window();
        let rows = h.find_id(w, "rows").unwrap();
        let top = if table { rows.y0 + 24.0 + 1.0 } else { rows.y0 };
        let row = |i: usize| Point::new(rows.x0 + 60.0, top + 24.0 * i as f64 + 12.0);
        h.send(Msg::Select(vec![1, 2, 3]));
        // Pressed on the selection: it goes whole.
        press_and_move(&mut h, w, row(2), Vec2::new(30.0, 40.0));
        assert_eq!(h.program().dragged.borrow().last(), Some(&vec![1, 2, 3]), "table: {table}");
        assert_eq!(h.drags().len(), 1);
        assert!(format!("{:?}", h.drags()[0].1).contains("text/uri-list"));
        assert_eq!(h.program().selected, [1, 2, 3], "the selection stays");
        // Pressed on another row: it's selected, and goes alone.
        press_and_move(&mut h, w, row(6), Vec2::new(30.0, 0.0));
        assert_eq!(h.program().dragged.borrow().last(), Some(&vec![6]), "table: {table}");
        assert_eq!(h.program().selected, [6]);
        // A press on a selection of several that doesn't drag picks its row.
        h.send(Msg::Select(vec![1, 2, 3]));
        h.press(w, row(3), PointerButton::Left, 1);
        assert_eq!(h.program().selected, [1, 2, 3], "not on the press");
        h.release(w, row(3), PointerButton::Left);
        assert_eq!(h.program().selected, [3], "table: {table}");
        assert_eq!(h.drags().len(), 2);
    }
}
