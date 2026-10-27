//! A scroll view whose content grows after it's shown (a pane's section
//! arriving, another item chosen) scrolls to the new end.

use aizome_kit::prelude::*;
use aizome_kit::testing::Harness;

struct Growing {
    rows: usize,
}

#[derive(Clone, Debug)]
enum Msg {
    More(usize),
}

impl Program for Growing {
    type Msg = Msg;
    fn info(&self) -> AppInfo {
        AppInfo::new("org.aizome.Growing", "Growing", "Test")
    }
    fn launched(&mut self, cx: &mut Cx<Msg>) {
        cx.open_window(WindowSpec::new("Growing").size(300.0, 200.0));
    }
    fn update(&mut self, msg: Msg, _cx: &mut Cx<Msg>) {
        match msg {
            Msg::More(n) => self.rows += n,
        }
    }
    fn view(&self, _w: WindowKey) -> Element<Msg> {
        let rows: Vec<Element<Msg>> = (0..self.rows).map(|i| text(format!("Row {i}")).height(30.0).into()).collect();
        scroll(column(rows).width(Fill)).into()
    }
}

fn shows(h: &Harness<Growing>, w: WindowKey, label: &str) -> bool {
    h.inspected(w).iter().any(|e| e["text"] == label && e["visible"] == true)
}

#[test]
fn rows_added_after_showing_can_be_scrolled_to() {
    let mut h = Harness::new(Growing { rows: 10 });
    let w = h.window();
    h.scroll(w, Point::new(100.0, 100.0), Vec2::new(0.0, 2000.0));
    assert!(shows(&h, w, "Row 9"));
    h.send(Msg::More(10));
    h.scroll(w, Point::new(100.0, 100.0), Vec2::new(0.0, 2000.0));
    assert!(shows(&h, w, "Row 19"), "the new end is out of reach");
}
