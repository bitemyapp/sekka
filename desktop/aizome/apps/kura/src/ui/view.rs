//! What each page looks like.

use std::collections::HashSet;
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;

use aizome_kit::element::MsgMap;
use aizome_kit::hash::PaintHasher;
use aizome_kit::prelude::*;
use aizome_kit::sidestep_ui::StrokeStyle;
use aizome_kit::sidestep_ui::kurbo::Shape;
use aizome_kit::widget::{LayoutCx, PaintCx, Widget};
use aizome_kit::{Constraints, column, row};

use super::drag::{self, RowActions};
use super::sunburst::Burst;
use super::{IndexState, Kura, Msg, Page, Side, colors, suggest_view};
use crate::format;
use crate::kinds::{self, FileKind};
use crate::mounts::Mount;
use crate::nix;
use crate::scan::Scan;
use crate::tree::NodeId;
use std::time::Instant;

pub const SIDE: f64 = 340.0;
pub const COLLECTOR: f64 = 214.0;

pub fn window(k: &Kura) -> Element<Msg> {
    let body = match k.page {
        Page::Disks => disks(k),
        Page::Scanning => scanning(k),
        Page::Results => results(k),
    };
    column![body, status(k)].width(Length::Fill).height(Length::Fill).into()
}

fn caption(s: &str) -> Element<Msg> {
    text(s.to_owned()).mono().size(10.5).letter_spacing(1.2).caps().role(Role::Text3).into()
}

fn toolbar(items: Vec<Element<Msg>>) -> Element<Msg> {
    aizome_kit::toolbar(items)
}

fn status(k: &Kura) -> Element<Msg> {
    let left = match (&k.notice, k.tree()) {
        (Some(n), _) => n.clone(),
        (None, Some(t)) if k.page == Page::Results => {
            let n = t.node(k.center);
            format!(
                "{} — {} on disk, {}",
                format::path(&t.path(k.center)),
                format::size(n.alloc),
                format::plural(u64::from(n.items), "item", "items")
            )
        }
        _ => match &k.scanning {
            Some(s) => format!("Scanning {}", format::path(&s.root)),
            None => "Choose a disk or folder to see what fills it.".into(),
        },
    };
    let right = match &k.index {
        IndexState::None => String::new(),
        IndexState::Writing if k.busy > 0 => "Scanning what changed…".into(),
        IndexState::Writing => "Looking for what could be freed…".into(),
        IndexState::Written { json, reclaimable } => match (json.is_some(), *reclaimable > 0) {
            (true, true) => format!("About {} could be freed · index for agents written", format::size(*reclaimable)),
            (true, false) => "Index for agents written".into(),
            (false, true) => format!("About {} could be freed", format::size(*reclaimable)),
            (false, false) => String::new(),
        },
        IndexState::Failed(e) => format!("The index for agents wasn't written: {e}"),
    };
    aizome_kit::status_bar(vec![
        text(left)
            .size(11.5)
            .role(if k.notice.is_some() { Role::Text } else { Role::Text3 })
            .width(Length::Fill)
            .into(),
        aizome_kit::status_text(right),
    ])
}

// The disk picker.

fn disks(k: &Kura) -> Element<Msg> {
    let bar = toolbar(vec![
        text("Scan a disk or folder").semibold().into(),
        spacer().into(),
        button("Scan Home Folder").compact().icon(Icon::House).on_press(Msg::ScanHome).into(),
        button("Choose Folder…").compact().icon(Icon::FolderSearch).on_press(Msg::ChooseFolder).into(),
    ]);
    let mut list: Vec<Element<Msg>> = vec![caption("Disks")];
    if k.mounts.is_empty() {
        list.push(text("Looking for disks…").role(Role::Text3).into());
    }
    for m in &k.mounts {
        list.push(disk_card(m));
    }
    if k.has_store {
        list.push(space(8.0));
        list.push(caption("Nix"));
        list.push(store_card(k));
    }
    list.push(space(8.0));
    list.push(
        text(format!(
            "Every scan also writes {} for agents: what fills the disk, and what could be freed, how and how safely.",
            k.index_dir.as_ref().map(|d| format::path(&d.join("index.json"))).unwrap_or_else(|| "an index".into())
        ))
        .size(11.5)
        .role(Role::Text3)
        .wrap()
        .into(),
    );
    let body = scroll(aizome_kit::column(list).spacing(8.0).padding([16.0, 20.0]).max_width(760.0).width(Length::Fill))
        .width(Length::Fill)
        .height(Length::Fill);
    column![bar, container(body).fill(Tone::Surface).width(Length::Fill).height(Length::Fill)]
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

fn card(content: Element<Msg>) -> Element<Msg> {
    container(content).padding([10.0, 12.0]).fill(Tone::Raised).border(Edge::BevelLo).bevel().width(Length::Fill).into()
}

fn disk_card(m: &Mount) -> Element<Msg> {
    let detail = format!("{} · {} · {}", m.mount_point.display(), m.source, m.fs_type);
    let free = format!("{} free of {}", format::size(m.available_bytes), format::size(m.total_bytes));
    card(
        row![
            icon(Icon::HardDrive).size(24.0).role(Role::Accent),
            column![text(m.title()).semibold(), text(detail).size(11.5).role(Role::Text3)]
                .spacing(2.0)
                .width(Length::Fill),
            column![progress(m.used_fraction()).width(200.0), text(free).size(11.5).tabular().role(Role::Text2)]
                .spacing(4.0)
                .align(Align::End),
            button("Scan").on_press(Msg::Scan(m.mount_point.clone())).label(format!("Scan {}", m.title())),
        ]
        .spacing(12.0)
        .width(Length::Fill)
        .into(),
    )
}

fn store_card(k: &Kura) -> Element<Msg> {
    let detail = match (&k.store, k.measuring_store) {
        (Some(s), _) => format!("{} in {} store paths", format::size(s.bytes), format::count(s.paths)),
        (None, true) => "Measuring…".into(),
        (None, false) => "Its size is Nix's to tell (`nix path-info --all`).".into(),
    };
    card(
        row![
            icon(Icon::Snowflake).size(24.0).role(Role::Cyan),
            column![
                text("Nix store").semibold(),
                text(detail).size(11.5).role(Role::Text3),
                text("Store paths go when nothing needs them and the garbage is collected: Yukimi's Storage does that safely.")
                    .size(11.5)
                    .role(Role::Text3)
                    .wrap(),
            ]
            .spacing(2.0)
            .width(Length::Fill),
            button("Open Yukimi's Storage").on_press(Msg::OpenYukimi),
        ]
        .spacing(12.0)
        .width(Length::Fill)
        .into(),
    )
}

// A scan under way.

fn scanning(k: &Kura) -> Element<Msg> {
    let Some(s) = &k.scanning else { return spacer().into() };
    let p = &s.snapshot;
    let bar = toolbar(vec![
        icon_button(Icon::ChevronLeft).toolbar().on_press(Msg::Stop).tooltip("Stop and go back").label("Stop").into(),
        text(format!("Scanning {}", format::path(&s.root))).semibold().into(),
        spacer().into(),
    ]);
    let current = p
        .current
        .strip_prefix(&s.root)
        .map(|r| r.display().to_string())
        .unwrap_or_else(|_| p.current.display().to_string());
    let body = column![
        text(format::size(p.bytes)).size(34.0).weight(650.0).tabular(),
        text(format!("{} · {}", format::plural(p.files, "file", "files"), format::plural(p.dirs, "folder", "folders")))
            .tabular()
            .role(Role::Text2),
        progress_indeterminate().width(320.0),
        text(current).size(11.5).mono().role(Role::Text3).max_width(560.0),
        space(4.0),
        button("Stop").on_press(Msg::Stop),
    ]
    .spacing(10.0)
    .align(Align::Center);
    column![bar, container(body).center().fill(Tone::Surface).width(Length::Fill).height(Length::Fill)]
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

// The results.

fn results(k: &Kura) -> Element<Msg> {
    let Some(scan) = k.scan.clone() else { return spacer().into() };
    let tree = &scan.tree;
    let lineage = tree.lineage(k.center);
    let mut crumbs: Vec<Element<Msg>> = Vec::new();
    let skip = lineage.len().saturating_sub(5);
    if skip > 0 {
        crumbs.push(text("…").role(Role::Text3).into());
    }
    for (i, &n) in lineage.iter().enumerate().skip(skip) {
        let name = if n == tree.root() { format::path(tree.root_path()) } else { tree.name(n).into_owned() };
        if i > skip || skip > 0 {
            crumbs.push(icon(Icon::ChevronRight).size(12.0).role(Role::Faint).into());
        }
        if n == k.center {
            crumbs.push(text(name).semibold().into());
        } else {
            crumbs.push(button(name).toolbar().on_press(Msg::GoTo(n)).into());
        }
    }
    let up = tree.parent(k.center).is_some();
    let mut items: Vec<Element<Msg>> = vec![
        icon_button(Icon::HardDrive).toolbar().on_press(Msg::ShowDisks).tooltip("Disks (❖D)").label("Disks").into(),
        icon_button(Icon::ArrowUp)
            .toolbar()
            .on_press_maybe(up.then_some(Msg::Up))
            .tooltip("Enclosing folder (❖↑)")
            .label("Enclosing folder")
            .into(),
        divider().vertical().height(18.0).into(),
    ];
    items.extend(crumbs);
    items.push(spacer().into());
    items.push(suggest_view::toolbar_button(k));
    items.push(
        icon_button(Icon::RefreshCw).toolbar().on_press(Msg::Rescan).tooltip("Rescan (❖R)").label("Rescan").into(),
    );
    let bar = toolbar(items);

    let state = Burst {
        scan: scan.clone(),
        generation: k.generation,
        center: k.center,
        segs: k.segs.clone(),
        outer: k.outer_segs.clone(),
        transition: k.transition,
        progress: k.transition.and_then(|t| t.progress(Instant::now())),
        hover: k.hover,
        selected: k.selected,
        press: k.press,
        dragging: k.dragging.is_some(),
        drag_over: k.drag_over,
        can_up: up,
        origin: k.origin.clone(),
        zone: k.zone.clone(),
        labels: k.labels.clone(),
        marks: k.marks.clone(),
        highlight_gen: k.highlight_gen,
        hint: k.hint_index().map(|i| {
            let h = &k.analysis.hints[i];
            (h.title.clone(), h.frees())
        }),
        rings: k.rings.clone(),
        rings_gen: k.rings_gen,
        ask: k.ask.clone(),
    };
    let burst: Element<Msg> = aizome_kit::canvas(state, |c, bounds, st: &Burst, theme| st.draw(c, bounds, theme))
        .on_event(|ev, size, st: &Burst| st.event(ev, size))
        .parts(|st: &Burst, size| st.parts(size))
        .width(Length::Fill)
        .height(Length::Fill)
        .label("Disk usage rings")
        .id("sunburst")
        .into();

    let side: Element<Msg> = match k.side {
        Side::Contents => {
            column![side_switch(k), list_header(), nix_note(k, &scan), folder_list(k, scan.clone()), collector(k)]
                .width(SIDE)
                .height(Length::Fill)
                .into()
        }
        Side::Suggestions => {
            column![side_switch(k), suggest_view::panel(k)].width(suggest_view::WIDTH).height(Length::Fill).into()
        }
    };
    let main = row![
        container(burst).fill(Tone::Surface).width(Length::Fill).height(Length::Fill),
        container(side).fill(Tone::Chrome).edges(Edge::BevelLo, Sides::LEFT).height(Length::Fill),
    ]
    .align(Align::Start)
    .width(Length::Fill)
    .height(Length::Fill);
    column![bar, main].width(Length::Fill).height(Length::Fill).into()
}

fn list_header() -> Element<Msg> {
    container(row![caption("Name"), spacer(), caption("Size")].width(Length::Fill))
        .padding([0.0, 12.0])
        .align_y(Align::Center)
        .edges(Edge::Line, Sides::BOTTOM)
        .width(Length::Fill)
        .height(26.0)
        .into()
}

/// Inside the Nix store, a word about how it's cleaned.
fn nix_note(k: &Kura, scan: &Scan) -> Element<Msg> {
    let tree = &scan.tree;
    let here = tree.path(k.center);
    let store_here = nix::in_store(&here) || here == Path::new(nix::STORE);
    let store_below = !store_here && k.rows.iter().any(|&r| tree.path(r) == Path::new(nix::STORE));
    if !(store_here || store_below) {
        return space(0.0);
    }
    container(
        column![
            row![icon(Icon::Snowflake).size(14.0).role(Role::Cyan), text("The Nix store").semibold().size(12.0)]
                .spacing(6.0),
            text("Store paths go when Yukimi collects the garbage, never by hand.").size(11.5).role(Role::Text2).wrap(),
            button("Open Yukimi's Storage").compact().on_press(Msg::OpenYukimi),
        ]
        .spacing(6.0),
    )
    .padding([8.0, 12.0])
    .fill(Tone::Sunk)
    .edges(Edge::Line, Sides::BOTTOM)
    .width(Length::Fill)
    .into()
}

fn kind_icon(tree: &crate::tree::Tree, n: NodeId) -> Icon {
    let node = tree.node(n);
    if node.is_dir() {
        return Icon::Folder;
    }
    if node.kind == crate::tree::Kind::Symlink {
        return Icon::Link;
    }
    match kinds::classify(tree.name_bytes(n), false) {
        FileKind::Video => Icon::Film,
        FileKind::Audio => Icon::EighthNotes,
        FileKind::Image => Icon::FileImage,
        FileKind::Archive => Icon::Archive,
        FileKind::DiskImage => Icon::Disc,
        FileKind::Code => Icon::FileCode,
        FileKind::Database => Icon::Database,
        FileKind::Document | FileKind::Log => Icon::FileText,
        _ => Icon::File,
    }
}

fn folder_list(k: &Kura, scan: Arc<Scan>) -> Element<Msg> {
    let rows = k.rows.clone();
    let collected: Rc<HashSet<NodeId>> = Rc::new(k.collector.iter().copied().collect());
    let total = scan.tree.node(k.center).alloc.max(1);
    let actions = RowActions {
        zone: k.zone.clone(),
        on_select: MsgMap::new(Msg::Select),
        on_open: MsgMap::new(Msg::Open),
        on_drag: MsgMap::new(Msg::Drag),
    };
    let selected = k.selected.and_then(|s| rows.iter().position(|&r| r == s));
    let n = rows.len();
    let (rows2, scan2) = (rows.clone(), scan.clone());
    let center = k.center;
    let collector: Rc<Vec<NodeId>> = Rc::new(k.collector.clone());
    let make_row = move |i: usize| -> Element<Msg> {
        let tree = &scan.tree;
        let node = rows[i];
        let nd = tree.node(node);
        let name = tree.name(node).into_owned();
        let draggable = !nix::in_store(&tree.path(node));
        let mark: Element<Msg> = if collected.contains(&node) {
            icon(Icon::Inbox).size(12.0).role(Role::Focus).tooltip("In the Collector").into()
        } else {
            space(0.0)
        };
        let content = row![
            Element::new(Swatch { branch: i as u16, is_dir: nd.is_dir() }, Vec::new()).width(3.0).height(16.0),
            icon(kind_icon(tree, node)).size(14.0),
            text(name).width(Length::Fill),
            mark,
            text(format::percent(nd.alloc, total)).size(11.0).tabular().role(Role::Text3),
            text(format::size(nd.alloc))
                .size(11.5)
                .tabular()
                .role(Role::Text2)
                .width(64.0)
                .align(aizome_kit::sidestep_ui::Alignment::Right),
        ]
        .spacing(6.0)
        .padding([0.0, 8.0])
        .width(Length::Fill)
        .height(24.0);
        drag::handle(node, draggable, &actions, content)
    };
    let list = virtual_list(n, 24.0, make_row)
        .selected(selected)
        .on_select(Msg::SelectRows)
        .on_activate(Msg::ActivateRow)
        .context_menu(move |i| menu_for(&scan2.tree, rows2[i], center, &collector))
        .type_select({
            let (rows, scan) = (k.rows.clone(), k.scan.clone());
            move |i| scan.as_ref().map(|s| s.tree.name(rows[i]).into_owned()).unwrap_or_default()
        })
        .id("folder-list")
        .label("Folder contents")
        .width(Length::Fill)
        .height(Length::Fill);
    list.into()
}

/// An item's menu (the list's, and the program's for the sunburst).
pub fn menu_for(tree: &crate::tree::Tree, node: NodeId, center: NodeId, collector: &[NodeId]) -> Menu<Msg> {
    let path = tree.path(node);
    let mut m = Menu::new(tree.name(node).into_owned());
    let n = tree.node(node);
    if n.is_dir() && n.count > 0 && node != center {
        m = m.item(MenuItem::new("Open", Msg::Open(node)));
    }
    if nix::in_store(&path) || path == Path::new(nix::STORE) {
        m = m.item(MenuItem::new("Open Yukimi's Storage", Msg::OpenYukimi).icon(Icon::Snowflake));
    } else if collector.contains(&node) {
        m = m.item(MenuItem::new("Remove from Collector", Msg::Uncollect(node)));
    } else if node != tree.root() {
        m = m.item(MenuItem::new("Collect", Msg::Collect(node)).icon(Icon::Inbox));
    }
    m.separator()
        .item(MenuItem::new("Reveal in Files", Msg::Reveal(node)).icon(Icon::FolderOpen))
        .item(MenuItem::new("Copy Path", Msg::CopyPath(node)).icon(Icon::Copy))
}

fn collector(k: &Kura) -> Element<Msg> {
    let tree = k.tree();
    let total = k.collected_bytes();
    let header = row![
        icon(Icon::Inbox).size(14.0).role(Role::Text3),
        caption("Collector"),
        spacer(),
        text(if k.collector.is_empty() { String::new() } else { format::size(total) }).semibold().tabular(),
    ]
    .spacing(6.0)
    .width(Length::Fill)
    .height(24.0);
    let target = k.dragging.is_some();
    let body: Element<Msg> = if k.collector.is_empty() || target {
        let words = if k.drag_over {
            "Let go to collect it"
        } else if target {
            "Drop here to collect"
        } else {
            "Drag items here, or right-click one and choose Collect"
        };
        Element::new(
            Dashed { lit: k.drag_over, target },
            vec![
                container(text(words).size(11.5).role(if target { Role::Text } else { Role::Text3 }).center().wrap())
                    .center()
                    .padding(8.0)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .into(),
            ],
        )
        .width(Length::Fill)
        .height(Length::Fill)
    } else {
        let t = tree.expect("a tree with collected items");
        let items: Vec<Element<Msg>> = k
            .collector
            .iter()
            .map(|&n| {
                row![
                    icon(kind_icon(t, n)).size(14.0),
                    text(t.name(n).into_owned()).width(Length::Fill).tooltip(format::path(&t.path(n))),
                    text(format::size(t.node(n).alloc)).size(11.5).tabular().role(Role::Text2),
                    icon_button(Icon::X).toolbar().on_press(Msg::Uncollect(n)).tooltip("Take out").label("Take out"),
                ]
                .spacing(6.0)
                .width(Length::Fill)
                .height(24.0)
                .into()
            })
            .collect();
        scroll(aizome_kit::column(items).width(Length::Fill)).width(Length::Fill).height(Length::Fill).into()
    };
    let has = !k.collector.is_empty();
    let buttons = row![
        button("Clear").compact().on_press_maybe(has.then_some(Msg::ClearCollector)),
        spacer(),
        button("Move to Trash…").compact().destructive().icon(Icon::Trash).on_press_maybe(has.then_some(Msg::AskTrash)),
    ]
    .width(Length::Fill);
    drag::drop_zone(
        k.zone.clone(),
        container(column![header, body, buttons].spacing(6.0).width(Length::Fill).height(Length::Fill))
            .padding([6.0, 12.0, 10.0, 12.0])
            .edges(Edge::BevelLo, Sides::TOP)
            .width(Length::Fill)
            .height(COLLECTOR),
    )
    .id("collector")
}

/// A list row's colour: its segment's.
struct Swatch {
    branch: u16,
    is_dir: bool,
}

impl Widget for Swatch {
    fn name(&self) -> &'static str {
        "kura-swatch"
    }

    fn layout(&mut self, _cx: &mut LayoutCx, bc: Constraints) -> Size {
        bc.constrain(Size::new(3.0, 16.0))
    }

    fn hash(&self, h: &mut PaintHasher) {
        h.add(&self.branch).add(&self.is_dir);
    }

    fn paint(&self, cx: &mut PaintCx) {
        let c = colors::segment(cx.theme(), self.branch, 1, self.is_dir);
        let b = cx.bounds();
        cx.canvas.fill_rect(b, c);
    }
}

/// The Collector's empty well, dashed as the Shelf's "Drop to park".
struct Dashed {
    lit: bool,
    target: bool,
}

impl Widget for Dashed {
    fn name(&self) -> &'static str {
        "kura-dashed"
    }

    fn layout(&mut self, cx: &mut LayoutCx, bc: Constraints) -> Size {
        let s = cx.layout_child(0, bc);
        cx.place_child(0, Point::ZERO);
        bc.constrain(s)
    }

    fn hash(&self, h: &mut PaintHasher) {
        h.add(&self.lit).add(&self.target);
    }

    fn paint(&self, cx: &mut PaintCx) {
        let t = cx.theme();
        let b = cx.bounds().inset(-0.5);
        if self.lit {
            cx.canvas.fill_rect(cx.bounds(), t.selection().with_alpha(0.35));
        } else if self.target {
            cx.canvas.fill_rect(cx.bounds(), t.sunk());
        }
        let edge = if self.target { t.focus() } else { t.line2() };
        let style = StrokeStyle::new(1.0).with_dash(vec![4.0, 3.0], 0.0);
        cx.canvas.stroke(&b.to_path(0.1), &style, edge);
    }
}

/// Above the side panel: the folder's contents, or the suggestions.
fn side_switch(k: &Kura) -> Element<Msg> {
    let n = k.analysis.hints.len();
    let label = if n == 0 { "Suggestions".to_owned() } else { format!("Suggestions ({n})") };
    container(
        segmented(["Contents".to_owned(), label], Some(if k.side == Side::Contents { 0 } else { 1 }))
            .on_select(|i| Msg::ShowSide(if i == 0 { Side::Contents } else { Side::Suggestions }))
            .width(Length::Fill),
    )
    .padding([6.0, 10.0])
    .edges(Edge::Line, Sides::BOTTOM)
    .width(Length::Fill)
    .into()
}
