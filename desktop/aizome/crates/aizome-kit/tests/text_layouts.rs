//! Text is laid out once for as long as it shows: however many labels a
//! window has, building its view again lays out only the ones that
//! changed.

use aizome_kit::prelude::*;
use aizome_kit::testing::Harness;
use aizome_kit::widgets::layout::column;

/// More labels than a cache of layouts nothing shows would keep.
const ROWS: usize = 6_000;

struct Rows {
    /// How many times each row's label has changed.
    changes: Vec<u32>,
}

#[derive(Clone, Debug)]
enum Msg {
    Change(usize),
    Nothing,
}

impl Program for Rows {
    type Msg = Msg;

    fn info(&self) -> AppInfo {
        AppInfo::new("org.aizome.Rows", "Rows", "Test")
    }

    fn launched(&mut self, cx: &mut Cx<Msg>) {
        cx.open_window(WindowSpec::new("Rows").size(400.0, 300.0));
    }

    fn update(&mut self, msg: Msg, _cx: &mut Cx<Msg>) {
        if let Msg::Change(row) = msg {
            self.changes[row] += 1;
        }
    }

    fn view(&self, _w: WindowKey) -> Element<Msg> {
        let rows = self.changes.iter().enumerate().map(|(i, n)| text(format!("row {i}, change {n}")).into());
        scroll(column(rows.collect())).into()
    }
}

#[test]
fn a_view_built_again_lays_out_only_what_changed() {
    aizome_kit::fonts::install();
    let mut h = Harness::new(Rows { changes: vec![0; ROWS] });
    let made = h.text_layouts().made;
    assert_eq!(made, ROWS as u64, "each label once");

    // Nothing changed: nothing is laid out, though there are more labels
    // than layouts are kept for once nothing shows them.
    h.send(Msg::Nothing);
    h.send(Msg::Nothing);
    assert_eq!(h.text_layouts().made, made);

    // One label changed: one layout, wherever in the window it is.
    for (n, row) in [0, ROWS / 2, ROWS - 1].into_iter().enumerate() {
        h.send(Msg::Change(row));
        assert_eq!(h.text_layouts().made, made + n as u64 + 1, "row {row}");
    }
}

#[test]
fn labels_new_together_are_laid_out_together() {
    aizome_kit::fonts::install();
    // A window opening shows labels nothing has shown. The first one laid
    // out takes the new ones after it along (Sidestep shares them out to
    // its text threads), so each of the others finds its layout kept.
    let h = Harness::new(Rows { changes: vec![0; 700] });
    let layouts = h.text_layouts();
    assert_eq!(layouts.made, 700);
    assert!(layouts.found >= 698, "{layouts:?}");
}
