//! The window: the toolbar, the drives, the selected disk's map and
//! table, the pending operations and the status line.

use std::rc::Rc;

use aizome_kit::hash::PaintHasher;
use aizome_kit::layout::Constraints;
use aizome_kit::prelude::*;
use aizome_kit::widget::{LayoutCx, PaintCx, Widget};
use aizome_kit::{column, row};
use aizome_sys::disks::format_size;

use super::glyphs;
use super::map::partition_map;
use crate::app::{Msg, Row, Tansu};
use crate::fs;
use crate::layout::{Layout, Mark, SegKind};
use crate::plan::{Action, Loss};

pub fn window(t: &Tansu) -> Element<Msg> {
    let body: Element<Msg> = if !t.state.available {
        unavailable()
    } else {
        sidebar_layout(sidebar(t), content(t)).first_size(248.0).into()
    };
    let mut parts = vec![toolbar(t), body];
    if t.state.available {
        if t.show_pending && !t.plan.is_empty() {
            parts.push(pending_list(t));
        }
        // Always there, so queueing the first operation (or undoing the
        // last) doesn't move everything above it.
        parts.push(pending_strip(t));
    }
    parts.push(status(t));
    aizome_kit::widgets::layout::column(parts).width(Length::Fill).height(Length::Fill).into()
}

fn unavailable() -> Element<Msg> {
    container(
        column![
            icon(Icon::HardDrive).size(40.0).role(Role::Faint),
            text("UDisks2 isn't running").size(17.0).semibold(),
            text("Tansu sees and changes disks through UDisks2. On NixOS, set services.udisks2.enable = true and rebuild.")
                .role(Role::Text2)
                .wrap()
                .center()
                .max_width(420.0),
        ]
        .spacing(10.0)
        .align(Align::Center),
    )
    .center()
    .fill(Tone::Surface)
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

/// A toolbar button for an action: enabled when it can be done, its
/// tooltip saying why not when it can't.
fn action_button(t: &Tansu, label: Option<&str>, icon: Icon, action: Action, tip: &str) -> Element<Msg> {
    let can = t.can(action);
    let b = match label {
        Some(l) => button(l.to_owned()).icon(icon),
        None => icon_button(icon),
    }
    .toolbar()
    .on_press_maybe(can.is_ok().then_some(Msg::Do(action)));
    let tip = match can {
        Ok(_) => tip.to_owned(),
        Err(e) => format!("{tip} — {e}"),
    };
    b.tooltip(tip).label(label.unwrap_or(tip_name_of(action))).into()
}

fn tip_name_of(a: Action) -> &'static str {
    match a {
        Action::Mount => "Mount",
        Action::Unmount => "Unmount",
        Action::Unlock => "Unlock",
        Action::Lock => "Lock",
        Action::SwapOn => "Use as swap",
        Action::SwapOff => "Stop using as swap",
        Action::New => "New partition",
        Action::Delete => "Delete",
        Action::Format => "Format",
        Action::Resize => "Resize",
        Action::Label => "Label",
        Action::Flags => "Flags",
        Action::Check => "Check",
        Action::Repair => "Repair",
    }
}

fn toolbar(t: &Tansu) -> Element<Msg> {
    let seg = t.selected().and_then(|i| t.layout.segs.get(i));
    let mounted = seg.is_some_and(|s| s.shown_mount().is_some());
    let swap = seg.is_some_and(|s| s.fs == "swap");
    let unlocked = seg.is_some_and(|s| s.is_unlocked());
    // As wide whatever it says, so the buttons after it stay put.
    let mount = match (swap, seg.is_some_and(|s| s.in_use)) {
        (true, true) => action_button(t, Some("Swap Off"), Icon::ToggleLeft, Action::SwapOff, "Stop using it as swap"),
        (true, false) => action_button(t, Some("Swap On"), Icon::ToggleRight, Action::SwapOn, "Use it as swap"),
        _ if mounted => action_button(t, Some("Unmount"), Icon::LogOut, Action::Unmount, "Unmount the filesystem"),
        _ => action_button(t, Some("Mount"), Icon::HardDriveDownload, Action::Mount, "Mount the filesystem"),
    };
    let mount: Element<Msg> = container(mount).width(98.0).into();
    let lock = if unlocked {
        action_button(t, None, Icon::Lock, Action::Lock, "Lock the encrypted volume")
    } else {
        action_button(t, None, Icon::LockOpen, Action::Unlock, "Unlock the encrypted volume…")
    };
    let entry = t.entry();
    let idle = t.run.is_none();
    let mut items = vec![
        mount,
        lock,
        toolbar_separator(),
        action_button(t, Some("New"), Icon::Plus, Action::New, "Make a partition in the unallocated space…"),
        action_button(t, Some("Delete"), Icon::Trash, Action::Delete, "Delete the partition"),
        action_button(t, Some("Format"), Icon::Paintbrush, Action::Format, "Make a new filesystem…"),
        action_button(t, Some("Resize"), Icon::ArrowLeftRight, Action::Resize, "Move the partition's end…"),
        toolbar_separator(),
        action_button(t, None, Icon::Tag, Action::Label, "Label…"),
        action_button(t, None, Icon::Bookmark, Action::Flags, "Flags…"),
        action_button(t, None, Icon::CircleCheck, Action::Check, "Check the filesystem"),
        action_button(t, None, Icon::Wrench, Action::Repair, "Repair the filesystem"),
        toolbar_separator(),
        {
            let can = t.can_new_table();
            let tip = match &can {
                Ok(()) => "New partition table…".to_owned(),
                Err(e) => format!("New partition table — {e}"),
            };
            icon_button(Icon::Table2)
                .toolbar()
                .on_press_maybe(can.is_ok().then_some(Msg::NewTable))
                .tooltip(tip)
                .label("New partition table")
                .into()
        },
        spacer().into(),
        button("Attach Image")
            .icon(Icon::Disc3)
            .toolbar()
            .on_press(Msg::AttachImage)
            .tooltip("Attach a disk image (.iso, .img) as a drive…")
            .into(),
    ];
    if entry.is_some_and(|e| e.is_image()) {
        items.push(
            button("Detach")
                .icon(Icon::X)
                .toolbar()
                .on_press_maybe(idle.then_some(Msg::DetachImage))
                .tooltip("Detach the image")
                .into(),
        );
    } else {
        items.push(
            button("SMART")
                .icon(Icon::Activity)
                .toolbar()
                .on_press_maybe(entry.map(|_| Msg::ShowSmart))
                .tooltip("SMART data and self-tests")
                .into(),
        );
    }
    items.push(
        button("Power Off")
            .icon(Icon::Power)
            .toolbar()
            .on_press_maybe((idle && entry.is_some_and(|e| e.can_power_off)).then_some(Msg::PowerOff))
            .tooltip("Unmount everything on the drive and power it off, to unplug it")
            .into(),
    );
    aizome_kit::toolbar(items)
}

fn sidebar(t: &Tansu) -> Element<Msg> {
    let mut items: Vec<Element<Msg>> = vec![caption("Drives")];
    let selected = t.drive.as_deref();
    for e in t.drives.iter().filter(|e| !e.is_image()) {
        items.push(glyphs::drive_row(e, Some(e.key.as_str()) == selected, Msg::SelectDrive(e.key.clone())));
    }
    let images: Vec<_> = t.drives.iter().filter(|e| e.is_image()).collect();
    if !images.is_empty() {
        items.push(caption("Images"));
        for e in images {
            items.push(glyphs::drive_row(e, Some(e.key.as_str()) == selected, Msg::SelectDrive(e.key.clone())));
        }
    }
    scroll(aizome_kit::widgets::layout::column(items).width(Length::Fill))
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

fn content(t: &Tansu) -> Element<Msg> {
    let Some(e) = t.entry() else {
        return container(text("No drives.").role(Role::Text3))
            .center()
            .fill(Tone::Surface)
            .width(Length::Fill)
            .height(Length::Fill)
            .into();
    };
    let l = &t.layout;
    let table_words = match l.table.as_str() {
        "gpt" => "GUID partition table",
        "dos" => "Master boot record",
        _ if l.segs.iter().any(|s| s.kind() == SegKind::Whole) => "No partition table",
        _ => "Empty",
    };
    let mut facts =
        vec![format_size(e.size), e.kind.words().to_owned(), table_words.to_owned(), format!("/dev/{}", e.device)];
    if l.new_table {
        facts[2] = format!("{table_words} (pending)");
    }
    if e.read_only {
        facts.push("read-only".into());
    }
    let smart = t.state.drive(&e.key).and_then(|d| d.smart.as_ref());
    let mut health: Vec<Element<Msg>> = Vec::new();
    if !e.is_image() {
        let mut words = glyphs::health_words(e.health).to_owned();
        if let Some(c) = smart.and_then(|s| s.temperature_celsius) {
            words.push_str(&format!(" · {c:.0} °C"));
        }
        health.push(
            button(words)
                .plain()
                .compact()
                .on_press(Msg::ShowSmart)
                .tooltip("SMART data and self-tests")
                .label("SMART health")
                .into(),
        );
    }
    let header = row![
        glyphs::glyph(e.kind, 34.0),
        column![text(e.name.clone()).size(17.0).weight(650.0), text(facts.join(" · ")).role(Role::Text3).tabular(),]
            .spacing(3.0)
            .width(Length::Fill),
        row![glyphs::dot(e.health)].extend(health).spacing(2.0),
    ]
    .spacing(12.0)
    .width(Length::Fill);
    let selected = t.selected();
    let map = partition_map(t.layout.clone(), selected, Msg::SelectSeg, |(i, at)| Msg::Context(i, at));
    column![
        container(column![header, map, legend(l)].spacing(12.0).width(Length::Fill))
            .padding([16.0, 16.0, 12.0, 16.0])
            .width(Length::Fill),
        table_view(t),
        details(t),
    ]
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

/// A filesystem's colour, as a small square.
struct Swatch {
    fs: String,
    hatched: bool,
}

impl Widget for Swatch {
    fn name(&self) -> &'static str {
        "tansu-swatch"
    }

    fn layout(&mut self, _cx: &mut LayoutCx, bc: Constraints) -> Size {
        bc.constrain(Size::new(10.0, 10.0))
    }

    fn hash(&self, h: &mut PaintHasher) {
        h.str(&self.fs).add(&self.hatched);
    }

    fn paint(&self, cx: &mut PaintCx) {
        let t = cx.theme().clone();
        let b = cx.bounds();
        if self.hatched {
            cx.canvas.fill_rect(b, t.sunk());
            use aizome_kit::sidestep_ui::StrokeStyle;
            use aizome_kit::sidestep_ui::kurbo::Line;
            cx.canvas.save();
            cx.canvas.clip_rect(b);
            for k in 0..4 {
                let x = -10.0 + k as f64 * 6.0;
                cx.canvas.stroke(&Line::new((x, 10.0), (x + 10.0, 0.0)), &StrokeStyle::new(1.0), t.line2());
            }
            cx.canvas.restore();
        } else {
            cx.canvas.fill_rect(b, fs::color(&self.fs, &t));
        }
        use aizome_kit::sidestep_ui::kurbo::Shape;
        cx.canvas.stroke(&b.inset(-0.5).to_path(0.1), &aizome_kit::sidestep_ui::StrokeStyle::new(1.0), t.bevel_lo());
    }
}

fn swatch(fs: &str) -> Element<Msg> {
    Element::new(Swatch { fs: fs.to_owned(), hatched: false }, Vec::new())
}

fn legend(l: &Layout) -> Element<Msg> {
    let mut seen: Vec<String> = Vec::new();
    for s in l.segs.iter().filter(|s| matches!(s.kind(), SegKind::Partition | SegKind::Whole)) {
        let fs = if s.is_locked() { "crypto_LUKS".to_owned() } else { s.shown_fs().to_owned() };
        if !seen.contains(&fs) {
            seen.push(fs);
        }
    }
    let mut items: Vec<Element<Msg>> = Vec::new();
    for fs in &seen {
        items.push(row![swatch(fs), text(fs::name(fs)).size(11.5).role(Role::Text2)].spacing(5.0).into());
    }
    if l.segs.iter().any(|s| s.is_free() || s.kind() == SegKind::Blank) {
        items.push(
            row![
                Element::new(Swatch { fs: String::new(), hatched: true }, Vec::new()),
                text("Unallocated").size(11.5).role(Role::Text2)
            ]
            .spacing(5.0)
            .into(),
        );
    }
    if l.segs.iter().any(|s| s.shown_used().is_some()) {
        items.push(text("Darker: space in use").size(11.5).role(Role::Text3).into());
    }
    if l.segs.iter().any(|s| !s.pending.is_empty()) {
        items.push(text("Dashed: pending changes").size(11.5).role(Role::Text3).into());
    }
    aizome_kit::widgets::layout::row(items).spacing(14.0).into()
}

const COLUMNS: [(&str, f64, bool); 7] = [
    ("Device", 124.0, false),
    ("Filesystem", 132.0, false),
    ("Label", 104.0, false),
    ("Mount Point", 150.0, false),
    ("Size", 80.0, true),
    ("Used", 116.0, true),
    ("Flags", 112.0, false),
];

fn table_view(t: &Tansu) -> Element<Msg> {
    let columns: Vec<Column> = COLUMNS
        .iter()
        .map(|(title, w, end)| {
            let c = Column::new(*title).width(*w);
            if *end { c.align_end() } else { c }
        })
        .collect();
    let layout = t.layout.clone();
    let rows = t.rows.clone();
    let count = rows.len();
    let selected: Vec<usize> =
        t.selected().map(|s| rows.iter().position(|r| r.seg == s && !r.inner)).into_iter().flatten().collect();
    // Menus for each row, as they'd be if it were selected.
    let menus: Rc<Vec<Menu<Msg>>> = Rc::new(rows.iter().map(|r| t.context_menu_at(Some(r.seg))).collect());
    let names = {
        let (layout, rows) = (layout.clone(), rows.clone());
        move |i: usize| layout.segs[rows[i].seg].short_name()
    };
    table(columns, count, move |r, c| cell(&layout, rows[r], c))
        .selected(selected)
        .on_select(Msg::SelectRows)
        .context_menu(move |i| menus.get(i).cloned().unwrap_or_else(|| Menu::new("")))
        .type_select(names)
        .width(Length::Fill)
        .height(Length::Fill)
        .id("partitions")
        .into()
}

fn dash() -> Element<Msg> {
    text("—").role(Role::Faint).into()
}

fn cell(l: &Layout, r: Row, c: usize) -> Element<Msg> {
    let s = &l.segs[r.seg];
    let pending = |m: Mark| s.pending.contains(&m);
    if r.inner {
        let Some(inner) = s.crypt.as_ref().and_then(|c| c.unlocked.as_ref()) else { return dash() };
        return match c {
            0 => text(format!("  ↳ {}", inner.device.trim_start_matches("/dev/"))).role(Role::Text2).into(),
            1 => row![swatch(&inner.fs), text(fs::name(&inner.fs))].spacing(6.0).into(),
            2 if !inner.label.is_empty() => text(inner.label.clone()).into(),
            3 => inner.mount.clone().map_or_else(dash, |m| text(m).mono().size(12.0).into()),
            4 => text(format_size(inner.size)).tabular().role(Role::Text2).into(),
            5 => inner.used.map_or_else(dash, |u| text(used_words(u, inner.size)).tabular().into()),
            _ => dash(),
        };
    }
    match c {
        0 => {
            let name: Element<Msg> = if s.is_free() {
                text(if s.logical { "Unallocated (logical)" } else { "Unallocated" }).role(Role::Text3).italic().into()
            } else {
                text(s.short_name()).semibold().into()
            };
            let mut parts = vec![name];
            if !s.pending.is_empty() {
                let what: Vec<&str> = s
                    .pending
                    .iter()
                    .map(|m| match m {
                        Mark::New => "new",
                        Mark::Format => "format",
                        Mark::Resize => "resize",
                        Mark::Label => "label",
                        Mark::Flags => "flags",
                        Mark::Repair => "repair",
                    })
                    .collect();
                parts.push(text(what.join(", ")).size(11.0).role(Role::Warning).into());
            }
            aizome_kit::widgets::layout::row(parts).spacing(6.0).into()
        }
        1 => match s.kind() {
            SegKind::Free => dash(),
            SegKind::Blank => text("No partition table").role(Role::Text3).into(),
            SegKind::Extended => text("Extended").role(Role::Text2).into(),
            _ => {
                let (fs, words) = if s.is_locked() {
                    ("crypto_LUKS".to_owned(), "LUKS, locked".to_owned())
                } else if s.is_unlocked() {
                    ("crypto_LUKS".to_owned(), "LUKS, unlocked".to_owned())
                } else {
                    (s.fs.clone(), fs::name(&s.fs))
                };
                let t = text(words);
                let t = if pending(Mark::Format) || pending(Mark::New) { t.role(Role::Warning) } else { t };
                row![swatch(&fs), t].spacing(6.0).into()
            }
        },
        2 => {
            let label = if s.is_unlocked() { String::new() } else { s.label.clone() };
            if label.is_empty() {
                return if s.name.is_empty() { dash() } else { text(s.name.clone()).role(Role::Text3).italic().into() };
            }
            let t = text(label);
            if pending(Mark::Label) { t.role(Role::Warning).into() } else { t.into() }
        }
        3 => match (&s.mount, s.fs.as_str(), s.in_use) {
            (Some(m), _, _) => text(m.clone()).mono().size(12.0).into(),
            (None, "swap", true) => text("swap, in use").role(Role::Text2).into(),
            _ => dash(),
        },
        4 => {
            let t = text(format_size(s.size)).tabular();
            if pending(Mark::Resize) {
                t.role(Role::Warning).into()
            } else {
                t.role(if s.is_free() { Role::Text3 } else { Role::Text }).into()
            }
        }
        5 => s.used.map_or_else(dash, |u| text(used_words(u, s.size)).tabular().into()),
        _ => {
            let flags = s.flag_list(&l.table);
            if flags.is_empty() {
                return dash();
            }
            let t = text(flags.join(", ")).role(Role::Text2);
            if pending(Mark::Flags) { t.role(Role::Warning).into() } else { t.into() }
        }
    }
}

fn used_words(used: u64, size: u64) -> String {
    let pct = if size == 0 { 0.0 } else { used as f64 * 100.0 / size as f64 };
    format!("{} · {pct:.0}%", format_size(used))
}

fn pending_strip(t: &Tansu) -> Element<Msg> {
    let n = t.plan.len();
    let last = t.plan.ops.last().map(|o| t.plan.words(o, &t.state).title).unwrap_or_default();
    let destroys = t.plan.ops.iter().any(|o| matches!(t.plan.words(o, &t.state).loss, Loss::Destroys(_)));
    let idle = t.run.is_none();
    let some = n > 0;
    let summary: Element<Msg> = if some {
        row![
            badge(n.to_string()).role(if destroys { Role::Danger } else { Role::Accent }),
            text(if n == 1 { "pending operation" } else { "pending operations" }).semibold(),
            text(format!("Last: {last}")).size(12.0).role(Role::Text3).width(Length::Fill),
        ]
        .spacing(8.0)
        .width(Length::Fill)
        .into()
    } else {
        text("No pending operations: changes wait here until you apply them.")
            .size(12.0)
            .role(Role::Text3)
            .width(Length::Fill)
            .into()
    };
    container(
        row![
            summary,
            button(if t.show_pending { "Hide List" } else { "Show List" })
                .compact()
                .on_press_maybe(some.then_some(Msg::TogglePending)),
            button("Undo")
                .compact()
                .icon(Icon::Undo2)
                .on_press_maybe((idle && some).then_some(Msg::Undo))
                .tooltip("Undo the last change to the plan (❖Z)"),
            button("Clear").compact().on_press_maybe((idle && some).then_some(Msg::ClearPlan)),
            button("Apply…")
                .compact()
                .primary()
                .on_press_maybe((idle && some).then_some(Msg::Apply))
                .tooltip("Review and run the operations (❖↩)"),
        ]
        .spacing(8.0)
        .width(Length::Fill),
    )
    .padding([5.0, 12.0])
    .fill(Tone::Chrome)
    .edges(Edge::BevelLo, Sides::TOP)
    .width(Length::Fill)
    .id("pending-strip")
    .into()
}

fn pending_list(t: &Tansu) -> Element<Msg> {
    let mut rows: Vec<Element<Msg>> = Vec::new();
    for (k, op) in t.plan.ops.iter().enumerate() {
        let w = t.plan.words(op, &t.state);
        let (icon_el, role) = match &w.loss {
            Loss::Destroys(_) => (Icon::TriangleAlert, Role::Danger),
            Loss::Risk(_) => (Icon::TriangleAlert, Role::Warning),
            Loss::None => (Icon::Dot, Role::Text3),
        };
        rows.push(
            row![
                text(format!("{}", k + 1)).mono().size(11.5).role(Role::Text3).width(18.0),
                icon(icon_el).size(14.0).role(role),
                text(w.title).width(Length::Fill),
                text(w.detail).size(11.5).role(Role::Text3).max_width(380.0),
            ]
            .spacing(8.0)
            .width(Length::Fill)
            .into(),
        );
    }
    let many = rows.len() > 6;
    let col = aizome_kit::widgets::layout::column(rows).spacing(6.0).padding([8.0, 12.0]).width(Length::Fill);
    let inner: Element<Msg> = if many { scroll(col).width(Length::Fill).height(170.0).into() } else { col.into() };
    container(inner).fill(Tone::Sunk).edges(Edge::BevelLo, Sides::TOP).width(Length::Fill).into()
}

fn status(t: &Tansu) -> Element<Msg> {
    let (left, error) = match &t.notice {
        Some((n, e)) => (n.clone(), *e),
        None => (selection_words(t), false),
    };
    let right = if t.state.available {
        let jobs = t.state.jobs.len();
        if jobs > 0 {
            format!("{jobs} job{} running · UDisks {}", if jobs == 1 { "" } else { "s" }, t.state.version)
        } else {
            format!("UDisks {}", t.state.version)
        }
    } else {
        String::new()
    };
    aizome_kit::status_bar(vec![
        text(left).size(11.5).role(if error { Role::Danger } else { Role::Text3 }).width(Length::Fill).into(),
        aizome_kit::status_text(right),
    ])
}

fn selection_words(t: &Tansu) -> String {
    let Some(s) = t.selected().and_then(|i| t.layout.segs.get(i)) else {
        return "Select a drive, then a partition.".into();
    };
    match s.kind() {
        SegKind::Free => {
            format!("{} unallocated at {}: make a partition here with New.", format_size(s.size), format_size(s.offset))
        }
        SegKind::Blank => "No partition table: create one (Drive ▸ New Partition Table…).".into(),
        SegKind::Extended => format!("{}: an extended partition holding logical ones.", s.short_name()),
        _ => {
            let mut words = format!("{} · {} · {}", s.short_name(), fs::name(s.shown_fs()), format_size(s.size));
            if let Some(m) = s.shown_mount() {
                words.push_str(&format!(" · mounted at {m}"));
            }
            if let Some(tid) =
                (!s.type_id.is_empty()).then(|| aizome_sys::disks::types::partition_type_name(&s.type_id)).flatten()
            {
                words.push_str(&format!(" · {tid}"));
            }
            words
        }
    }
}

/// What the table doesn't say about the selection: types, identifiers,
/// where it starts, and how full it is.
const DETAILS_HEIGHT: f64 = 124.0;

fn details(t: &Tansu) -> Element<Msg> {
    let Some(i) = t.selected() else { return space(DETAILS_HEIGHT) };
    let s = &t.layout.segs[i];
    let block = s.block().and_then(|p| t.state.block(p));
    let pair = |k: &str, v: String| -> Element<Msg> {
        row![
            text(k.to_owned()).size(11.5).role(Role::Text3).width(96.0),
            text(v).size(12.0).role(Role::Text2).selectable().width(Length::Fill)
        ]
        .spacing(8.0)
        .width(Length::Fill)
        .into()
    };
    let exact = |b: u64| {
        let digits = b.to_string();
        let mut out = String::new();
        for (k, c) in digits.chars().enumerate() {
            if k > 0 && (digits.len() - k).is_multiple_of(3) {
                out.push('\u{2009}');
            }
            out.push(c);
        }
        out
    };
    let mut left: Vec<Element<Msg>> = Vec::new();
    let mut right: Vec<Element<Msg>> = Vec::new();
    let title = match s.kind() {
        SegKind::Free => "Unallocated space".to_owned(),
        SegKind::Blank => "Blank disk".to_owned(),
        _ if matches!(s.target, Some(crate::layout::Target::New(_))) => "New partition (pending)".to_owned(),
        _ => s.short_name(),
    };
    left.push(pair("Starts at", format!("{} ({} bytes)", format_size(s.offset), exact(s.offset))));
    left.push(pair("Size", format!("{} ({} bytes)", format_size(s.size), exact(s.size))));
    if !s.type_id.is_empty() {
        let name = aizome_sys::disks::types::partition_type_name(&s.type_id).unwrap_or("Unknown type");
        left.push(pair("Type", format!("{name} · {}", s.type_id)));
    }
    if let Some(b) = block {
        if !b.id_uuid.is_empty() {
            right.push(pair("UUID", b.id_uuid.clone()));
        }
        if let Some(p) = b.partition.as_ref().filter(|p| !p.uuid.is_empty()) {
            right.push(pair("Partition UUID", p.uuid.clone()));
        }
        if let Some(e) = &b.encrypted {
            right.push(pair(
                "Encryption",
                if e.hint_type.is_empty() { "LUKS".into() } else { e.hint_type.to_uppercase() },
            ));
        }
        if let Some(l) = &b.loop_device {
            right.push(pair("Image file", l.backing_file.clone()));
        }
    }
    if !s.name.is_empty() {
        left.push(pair("Name", s.name.clone()));
    }
    if !s.pending.is_empty() {
        let what: Vec<&str> = s
            .pending
            .iter()
            .map(|m| match m {
                Mark::New => "made",
                Mark::Format => "formatted",
                Mark::Resize => "resized",
                Mark::Label => "labelled",
                Mark::Flags => "flagged",
                Mark::Repair => "repaired",
            })
            .collect();
        right.push(pair("Pending", format!("To be {} when you apply.", what.join(", "))));
    }
    match s.shown_mount() {
        _ if !s.pending.is_empty() => {}
        Some(m) => {
            let total = s.crypt.as_ref().and_then(|c| c.unlocked.as_ref()).map_or(s.size, |i| i.size);
            let used = s.shown_used().unwrap_or(0);
            right.push(
                row![
                    text("In use").size(11.5).role(Role::Text3).width(96.0),
                    progress(if total == 0 { 0.0 } else { used as f64 / total as f64 }).width(140.0),
                    text(format!("{} of {} · at {m}", format_size(used), format_size(total)))
                        .size(12.0)
                        .role(Role::Text2)
                        .width(Length::Fill),
                ]
                .spacing(8.0)
                .width(Length::Fill)
                .into(),
            );
        }
        None if matches!(s.kind(), SegKind::Partition | SegKind::Whole)
            && !s.shown_fs().is_empty()
            && s.shown_fs() != "swap"
            && !s.is_locked() =>
        {
            right.push(pair("In use", "Mount it to see how full it is.".into()));
        }
        _ => {}
    }
    container(
        column![
            text(title).semibold(),
            row![
                aizome_kit::widgets::layout::column(left).spacing(4.0).width(Length::Fill),
                aizome_kit::widgets::layout::column(right).spacing(4.0).width(Length::Fill),
            ]
            .spacing(24.0)
            .align(Align::Start)
            .width(Length::Fill),
        ]
        .spacing(8.0)
        .width(Length::Fill),
    )
    .padding([10.0, 16.0, 12.0, 16.0])
    .fill(Tone::Chrome)
    .edges(Edge::BevelLo, Sides::TOP)
    .width(Length::Fill)
    // Always as tall, so the table above doesn't move (and redraw) as the
    // selection changes.
    .height(DETAILS_HEIGHT)
    .id("details")
    .into()
}
