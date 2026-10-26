//! Tables and outlines.

use std::collections::HashSet;
use std::sync::Arc;

use aizome_kit::column;
use aizome_kit::prelude::*;
use aizome_kit::sidestep_ui::{Modifiers, NamedKey};
use aizome_kit::testing::Harness;

struct Procs {
    rows: Arc<Vec<(String, u32, f64)>>,
    selected: Vec<usize>,
    sort: Option<(usize, bool)>,
}

#[derive(Clone, Debug)]
enum Msg {
    Select(Vec<usize>),
    Sort(usize, bool),
}

impl Program for Procs {
    type Msg = Msg;
    fn info(&self) -> AppInfo {
        AppInfo::new("org.aizome.Myaku", "Myaku", "Activity")
    }
    fn launched(&mut self, cx: &mut Cx<Msg>) {
        cx.open_window(WindowSpec::new("Processes").size(480.0, 240.0));
    }
    fn update(&mut self, msg: Msg, _cx: &mut Cx<Msg>) {
        match msg {
            Msg::Select(s) => self.selected = s,
            Msg::Sort(c, asc) => {
                self.sort = Some((c, asc));
                let mut v = (*self.rows).clone();
                v.sort_by(|a, b| {
                    let o = match c {
                        0 => a.0.cmp(&b.0),
                        1 => a.1.cmp(&b.1),
                        _ => a.2.total_cmp(&b.2),
                    };
                    if asc { o } else { o.reverse() }
                });
                self.rows = Arc::new(v);
            }
        }
    }
    fn view(&self, _w: WindowKey) -> Element<Msg> {
        let rows = self.rows.clone();
        let mut t = table(
            vec![
                Column::new("Process").width(220.0).sortable(),
                Column::new("PID").width(80.0).align_end().sortable(),
                Column::new("CPU").align_end().sortable(),
            ],
            rows.len(),
            move |r, c| match c {
                0 => text(rows[r].0.clone()),
                1 => text(rows[r].1.to_string()).mono().tabular().role(Role::Text2),
                _ => text(format!("{:.1}%", rows[r].2)).mono().tabular(),
            },
        )
        .selected(self.selected.iter().copied())
        .on_select(Msg::Select)
        .on_sort(Msg::Sort)
        .multiple()
        .id("table");
        if let Some((c, a)) = self.sort {
            t = t.sorted(c, a);
        }
        column![t].into()
    }
}

fn procs() -> Procs {
    let rows =
        (0..1000).map(|i| (format!("process-{:04}", (i * 7919) % 1000), 1000 + i, (i % 97) as f64 / 3.0)).collect();
    Procs { rows: Arc::new(rows), selected: Vec::new(), sort: None }
}

#[test]
fn tables_build_only_rows_in_sight_and_sort_by_their_headers() {
    let mut h = Harness::new(procs());
    let w = h.window();
    let cells = h.texts(w).iter().filter(|t| t.starts_with("process-")).count();
    assert!(cells <= 10, "{cells} rows built of 1000");
    // Sort by PID, then the other way.
    let t = h.find_id(w, "table").unwrap();
    h.click(w, Point::new(t.x0 + 260.0, t.y0 + 12.0));
    assert_eq!(h.program().sort, Some((1, true)));
    h.click(w, Point::new(t.x0 + 260.0, t.y0 + 12.0));
    assert_eq!(h.program().sort, Some((1, false)));
    assert!(h.texts(w).iter().any(|t| t == "1999"), "descending: the biggest PID first");
    // Rows by click and keys.
    h.focus_window(w, true);
    h.click(w, Point::new(t.x0 + 50.0, t.y0 + 24.0 + 12.0));
    assert_eq!(h.program().selected, [0]);
    h.named(w, NamedKey::ArrowDown, Modifiers::SHIFT);
    h.named(w, NamedKey::ArrowDown, Modifiers::SHIFT);
    assert_eq!(h.program().selected, [0, 1, 2]);
    h.named(w, NamedKey::End, Modifiers::NONE);
    assert_eq!(h.program().selected, [999]);
    let out = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("table.png");
    h.save_png(w, &out, 2.0);
}

#[test]
fn column_edges_drag() {
    let mut h = Harness::new(procs());
    let w = h.window();
    let t = h.find_id(w, "table").unwrap();
    let before = h.find(w, "1000").unwrap();
    h.drag(w, Point::new(t.x0 + 220.0, t.y0 + 12.0), Point::new(t.x0 + 300.0, t.y0 + 12.0), 4);
    let after = h.find(w, "1000").unwrap();
    assert!(after.x0 > before.x0 + 60.0, "the PID column moved right: {before:?} → {after:?}");
}

/// An outline the program keeps open or not.
struct Disks {
    items: Arc<Vec<TreeItem>>,
    open: Arc<HashSet<u64>>,
    selected: Vec<u64>,
    toggled: Vec<(u64, bool)>,
}

#[derive(Clone, Debug)]
enum DMsg {
    Toggle(u64, bool),
    Select(Vec<u64>),
}

impl Program for Disks {
    type Msg = DMsg;
    fn info(&self) -> AppInfo {
        AppInfo::new("org.aizome.Kura", "Kura", "Disk Usage")
    }
    fn launched(&mut self, cx: &mut Cx<DMsg>) {
        cx.open_window(WindowSpec::new("Kura").size(360.0, 240.0));
    }
    fn update(&mut self, msg: DMsg, _cx: &mut Cx<DMsg>) {
        match msg {
            DMsg::Toggle(id, on) => {
                self.toggled.push((id, on));
                let mut o = (*self.open).clone();
                if on {
                    o.insert(id);
                } else {
                    o.remove(&id);
                }
                self.open = Arc::new(o);
            }
            DMsg::Select(s) => self.selected = s,
        }
    }
    fn view(&self, _w: WindowKey) -> Element<DMsg> {
        column![
            tree(self.items.clone())
                .expanded(self.open.clone())
                .on_toggle(DMsg::Toggle)
                .selected(self.selected.iter().copied())
                .on_select(DMsg::Select)
                .id("tree")
        ]
        .into()
    }
}

fn disks() -> Disks {
    let items = vec![
        TreeItem::branch(
            1,
            "home",
            vec![
                TreeItem::branch(
                    2,
                    "callen",
                    vec![
                        TreeItem::leaf(3, "work").icon(Icon::Folder).detail("41.2 GB"),
                        TreeItem::leaf(4, "Downloads").icon(Icon::Download).detail("3.1 GB"),
                    ],
                )
                .icon(Icon::House)
                .detail("44.3 GB"),
            ],
        )
        .icon(Icon::HardDrive)
        .detail("44.3 GB"),
        TreeItem::branch(5, "nix", vec![TreeItem::leaf(6, "store").icon(Icon::Package).detail("88.0 GB")])
            .icon(Icon::HardDrive)
            .detail("88.0 GB"),
    ];
    Disks { items: Arc::new(items), open: Arc::new(HashSet::new()), selected: Vec::new(), toggled: Vec::new() }
}

#[test]
fn outlines_open_by_their_triangles_and_keys() {
    let mut h = Harness::new(disks());
    let w = h.window();
    assert!(!h.texts(w).iter().any(|t| t == "callen"));
    let t = h.find_id(w, "tree").unwrap();
    // The triangle of "home".
    h.click(w, Point::new(t.x0 + 10.0, t.y0 + 12.0));
    assert_eq!(h.program().toggled, [(1, true)]);
    assert!(h.texts(w).iter().any(|t| t == "callen"));
    // Select "callen", then → opens it, ← closes it, ← again goes up.
    h.focus_window(w, true);
    h.click(w, Point::new(t.x0 + 120.0, t.y0 + 24.0 + 12.0));
    assert_eq!(h.program().selected, [2]);
    h.named(w, NamedKey::ArrowRight, Modifiers::NONE);
    assert!(h.texts(w).iter().any(|t| t == "work"));
    let out = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("tree.png");
    h.save_png(w, &out, 2.0);
    h.named(w, NamedKey::ArrowLeft, Modifiers::NONE);
    assert!(!h.texts(w).iter().any(|t| t == "work"));
    h.named(w, NamedKey::ArrowLeft, Modifiers::NONE);
    assert_eq!(h.program().selected, [1]);
}
