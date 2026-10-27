//! The window: the toolbar (lenses, what to list, search), the table of
//! applications and processes, the inspector, the graphs, the status line.

use std::rc::Rc;
use std::sync::Arc;

use aizome_kit::hash::PaintHasher;
use aizome_kit::layout::Constraints;
use aizome_kit::prelude::*;
use aizome_kit::widget::{LayoutCx, PaintCx, Widget};
use aizome_kit::{column, row};

use super::columns::{self, Col};
use super::graph::{Hue, Scale, Style, graph};
use crate::app::{Msg, Myaku, RowRef, Show};
use crate::model::{self, Frame, Kind, Lens, Meter};

pub fn window(m: &Myaku) -> Element<Msg> {
    let mut middle: Vec<Element<Msg>> = vec![table_view(m)];
    if m.inspector {
        middle.push(divider().vertical().into());
        middle.push(super::inspector::view(m));
    }
    column![
        toolbar(m),
        aizome_kit::widgets::layout::row(middle).width(Length::Fill).height(Length::Fill),
        graphs(m),
        status(m),
    ]
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

fn toolbar(m: &Myaku) -> Element<Msg> {
    let lenses: Vec<Lens> = Lens::ALL.into_iter().filter(|l| m.lens_shown(*l)).collect();
    let current = lenses.iter().position(|l| *l == m.lens);
    let picks = lenses.clone();
    let can = m.can_quit();
    let tip = match &can {
        Ok(_) => "Quit the selection…".to_owned(),
        Err(e) => format!("Quit — {e}"),
    };
    let mut items: Vec<Element<Msg>> = vec![
        icon_button(Icon::Info)
            .toolbar()
            .selected(m.inspector)
            .on_press(Msg::Inspector(!m.inspector))
            .tooltip("Inspector (❖I)")
            .label("Inspector")
            .into(),
        button("Quit")
            .icon(Icon::CircleX)
            .toolbar()
            .on_press_maybe(can.is_ok().then_some(Msg::AskQuit(false)))
            .tooltip(tip)
            .into(),
        toolbar_separator(),
        segmented(lenses.iter().map(|l| l.title()), current).on_select(move |i| Msg::Lens(picks[i])).into(),
        spacer().into(),
    ];
    // With several cards, which the GPU lens shows: every card, or one.
    let gpus = &m.frame.gpus;
    if m.lens == Lens::Gpu && gpus.len() > 1 {
        let names = model::gpu_names(gpus);
        let order = model::gpu_order(gpus);
        let shown = order.iter().position(|&i| m.gpu.as_ref() == Some(&gpus[i].pci)).map_or(0, |at| at + 1);
        let pcis: Vec<String> = order.iter().map(|&i| gpus[i].pci.clone()).collect();
        items.push(
            dropdown(
                std::iter::once("All GPUs".to_owned()).chain(order.iter().map(|&i| names[i].clone())),
                Some(shown),
            )
            .compact()
            .on_select(move |i| Msg::Gpu(i.checked_sub(1).and_then(|i| pcis.get(i).cloned())))
            .id("gpu-selector")
            .label("GPU")
            .tooltip("Show the processes using every GPU, or one")
            .into(),
        );
    }
    items.extend([
        dropdown(
            ["Applications", "All Processes", "My Processes"],
            Some(match m.show {
                Show::Apps => 0,
                Show::All => 1,
                Show::Mine => 2,
            }),
        )
        .compact()
        .on_select(|i| Msg::Show([Show::Apps, Show::All, Show::Mine][i.min(2)]))
        .into(),
        search_field(&m.search).placeholder("Search").on_change(Msg::Search).id("search").width(190.0).into(),
    ]);
    aizome_kit::toolbar(items)
}

/// A kind's glyph and colour.
fn kind_icon(kind: Option<Kind>) -> (Icon, Role) {
    match kind {
        Some(Kind::App) => (Icon::AppWindow, Role::Accent),
        Some(Kind::Service) => (Icon::Settings2, Role::Text2),
        Some(Kind::System) => (Icon::Server, Role::Text3),
        Some(Kind::Kernel) | None => (Icon::Cpu, Role::Text3),
    }
}

fn dash() -> Element<Msg> {
    text("—").role(Role::Faint).into()
}

fn table_view(m: &Myaku) -> Element<Msg> {
    // Beside the inspector, narrower columns and no User, so the figures
    // fit without scrolling sideways.
    let narrow = m.inspector;
    let mut cols = m.columns();
    if narrow && cols.last() == Some(&Col::User) {
        cols.pop();
    }
    let columns: Vec<Column> = cols
        .iter()
        .map(|c| {
            let width = if narrow { c.narrow_width() } else { c.width() };
            let col = Column::new(c.title()).width(width);
            // The cards' names don't sort; % GPU and Video Memory do the work.
            let col = if *c == Col::Card { col } else { col.sortable() };
            if c.numeric() { col.align_end() } else { col }
        })
        .collect();
    let cols = Rc::new(cols);
    let (sort, asc) = m.sort();
    let sorted = cols.iter().position(|c| *c == sort).unwrap_or(0);
    let rows = m.rows.clone();
    let frame = m.frame.clone();
    let expanded: Rc<std::collections::HashSet<(u32, u64)>> = Rc::new(m.expanded.clone());
    let menus = {
        let (rows, frame) = (rows.clone(), frame.clone());
        move |i: usize| row_menu(&frame, rows.get(i).copied(), i)
    };
    let names = {
        let (rows, frame) = (rows.clone(), frame.clone());
        move |i: usize| match rows[i] {
            RowRef::Group(g) => frame.groups[g].name.clone(),
            RowRef::Process(p, _) => frame.processes[p].name.clone(),
        }
    };
    let count = rows.len();
    let gpu = m.gpu_filter();
    let cards = Rc::new(model::gpu_names(&frame.gpus));
    let selected: Vec<usize> = m.selected_row().into_iter().collect();
    let body = table(columns, count, move |r, c| cell(&frame, rows[r], cols[c], &expanded, gpu, &cards))
        .sorted(sorted, asc)
        .on_sort(Msg::Sort)
        .selected(selected)
        .on_select(Msg::SelectRows)
        .on_activate(Msg::Activate)
        .context_menu(menus)
        .type_select(names)
        .width(Length::Fill)
        .height(Length::Fill)
        .id("processes");
    if count == 0 && m.frames > 0 {
        let message = if let Some(device) = m.gpu_filter().filter(|_| m.search.is_empty()) {
            super::gpu::nothing(&m.frame, device)
        } else if m.search.is_empty() {
            "Nothing to list.".to_owned()
        } else {
            format!("Nothing matches “{}”.", m.search)
        };
        return container(text(message).role(Role::Text3))
            .center()
            .fill(Tone::Surface)
            .width(Length::Fill)
            .height(Length::Fill)
            .into();
    }
    if m.frames == 0 {
        return container(
            row![progress_indeterminate().width(120.0), text("Reading the system…").role(Role::Text3)].spacing(10.0),
        )
        .center()
        .fill(Tone::Surface)
        .width(Length::Fill)
        .height(Length::Fill)
        .into();
    }
    body.into()
}

fn row_menu(f: &Frame, r: Option<RowRef>, i: usize) -> Menu<Msg> {
    let Some(r) = r else { return Menu::new("") };
    let (own, kernel, paused) = match r {
        RowRef::Group(g) => {
            let g = &f.groups[g];
            (g.own, g.kind == Some(Kind::Kernel), g.members.iter().all(|&m| f.processes[m].paused()))
        }
        RowRef::Process(p, _) => {
            let p = &f.processes[p];
            (p.uid == f.uid || f.uid == 0, p.kernel, p.paused())
        }
    };
    let can = own && !kernel;
    Menu::new("")
        .item(MenuItem::new("Inspect", Msg::Activate(i)).shortcut(Shortcut::cmd('I')))
        .separator()
        .item(MenuItem::new("Quit…", Msg::AskQuit(false)).enabled(can))
        .item(MenuItem::new("Force Quit…", Msg::AskQuit(true)).enabled(can))
        .separator()
        .item(MenuItem::new(if paused { "Resume" } else { "Pause" }, Msg::Pause(!paused)).enabled(can))
}

fn cell(
    f: &Frame,
    r: RowRef,
    col: Col,
    expanded: &std::collections::HashSet<(u32, u64)>,
    gpu: Option<usize>,
    names: &[String],
) -> Element<Msg> {
    match (r, col) {
        (r, Col::Card) => {
            let used = match r {
                RowRef::Group(g) => super::gpu::used_by(f.groups[g].members.iter().map(|&m| &f.processes[m]), names),
                RowRef::Process(p, _) => super::gpu::used_by(std::iter::once(&f.processes[p]), names),
            };
            match used {
                Some(t) => text(t).role(Role::Text2).into(),
                None => dash(),
            }
        }
        (RowRef::Group(g), Col::Name) => {
            let grp = &f.groups[g];
            let (glyph, role) = kind_icon(grp.kind);
            let open = expanded.contains(&grp.key);
            let disclosure: Element<Msg> = if grp.members.len() > 1 {
                mouse_area(icon(if open { Icon::ChevronDown } else { Icon::ChevronRight }).size(12.0).role(Role::Text3))
                    .on_press(Msg::Toggle(grp.key))
                    .label(if open { "Hide its processes" } else { "Show its processes" })
                    .into()
            } else {
                space(12.0)
            };
            let mut parts = vec![
                disclosure,
                icon(glyph).size(14.0).role(role).into(),
                text(grp.name.clone()).weight(if grp.focused { 650.0 } else { 500.0 }).into(),
            ];
            let detail = if let Some(device) = gpu.filter(|_| grp.members.len() > 1) {
                let clients = grp.members.iter().filter(|&&p| f.processes[p].uses(device)).count();
                if clients == grp.members.len() {
                    model::count(clients, "process", "processes")
                } else {
                    format!("{clients} of {} processes", grp.members.len())
                }
            } else if !grp.detail.is_empty() && grp.detail != grp.name {
                grp.detail.clone()
            } else if grp.members.len() > 1 {
                model::count(grp.members.len(), "process", "processes")
            } else {
                String::new()
            };
            if !detail.is_empty() {
                parts.push(text(detail).size(11.5).role(Role::Text3).into());
            }
            aizome_kit::widgets::layout::row(parts).spacing(6.0).into()
        }
        (RowRef::Process(p, member), Col::Name) => {
            let p = &f.processes[p];
            let mut parts: Vec<Element<Msg>> = Vec::new();
            if member {
                parts.push(space(30.0));
            } else {
                let (glyph, role) = kind_icon(f.groups[p.group].kind);
                parts.push(icon(glyph).size(14.0).role(role).into());
            }
            let name = text(p.name.clone());
            parts.push(if p.paused() { name.italic().role(Role::Text3).into() } else { name.into() });
            let mut detail = p.role.clone();
            if p.paused() {
                detail = if detail.is_empty() { "paused".into() } else { format!("{detail}, paused") };
            }
            if !detail.is_empty() {
                parts.push(text(detail).size(11.5).role(Role::Text3).into());
            }
            aizome_kit::widgets::layout::row(parts).spacing(6.0).into()
        }
        (RowRef::Group(g), c) => {
            let grp = &f.groups[g];
            let first = &f.processes[grp.members[0]];
            let pid = (grp.members.len() == 1).then_some(first.pid);
            let user = Some(first.user.as_str());
            match columns::text(c, &grp.usage, pid, user) {
                Some(t) => text(t).tabular().role(if c == Col::User { Role::Text2 } else { Role::Text }).into(),
                None => dash(),
            }
        }
        (RowRef::Process(p, _), c) => {
            let p = &f.processes[p];
            match columns::text(c, &p.usage, Some(p.pid), Some(&p.user)) {
                Some(t) => text(t)
                    .tabular()
                    .role(if matches!(c, Col::User | Col::Pid) { Role::Text2 } else { Role::Text })
                    .into(),
                None => dash(),
            }
        }
    }
}

/// A coloured square for a graph's legend.
struct Swatch(Hue);

impl Widget for Swatch {
    fn name(&self) -> &'static str {
        "myaku-swatch"
    }

    fn layout(&mut self, _cx: &mut LayoutCx, bc: Constraints) -> Size {
        bc.constrain(Size::new(8.0, 8.0))
    }

    fn hash(&self, h: &mut PaintHasher) {
        h.add(&self.0);
    }

    fn paint(&self, cx: &mut PaintCx) {
        let c = self.0.color(cx.theme());
        let b = cx.bounds();
        cx.canvas.fill_rect(b, c);
    }
}

pub(super) fn key(hue: Hue, words: String) -> Element<Msg> {
    row![Element::new(Swatch(hue), Vec::new()), text(words).size(11.0).tabular().role(Role::Text2)].spacing(5.0).into()
}

/// A graph's place in the strip: room for a frame around it.
pub(super) fn slot(card: Element<Msg>) -> aizome_kit::widgets::layout::Container<Msg> {
    container(card).padding([5.0, 7.0]).width(Length::Fill)
}

pub(super) fn card(title: &str, value: String, body: Element<Msg>, keys: Vec<Element<Msg>>) -> Element<Msg> {
    column![
        row![
            text(title.to_owned()).semibold().size(12.0),
            spacer(),
            text(value).size(11.5).tabular().role(Role::Text2)
        ]
        .width(Length::Fill),
        body,
        aizome_kit::widgets::layout::row(keys).spacing(10.0),
    ]
    .spacing(5.0)
    .width(Length::Fill)
    .into()
}

fn graphs(m: &Myaku) -> Element<Msg> {
    let h = &m.history;
    let s = &m.frame.system;
    let mem = &s.memory;
    let strain = s.pressure.map(|p| match p.memory_strain() {
        aizome_sys::sysinfo::detail::Strain::Low => "pressure low",
        aizome_sys::sysinfo::detail::Strain::Moderate => "pressure moderate",
        aizome_sys::sysinfo::detail::Strain::High => "pressure high",
    });
    let cpu = card(
        "CPU",
        format!("{}%", model::percent(s.cpu)),
        graph(
            [h.cpu_user.clone(), h.cpu_system.clone()],
            [Hue::Blue, Hue::Violet],
            Style::Stacked,
            Scale::Fixed(100.0),
            "Processor use over two minutes".into(),
        ),
        vec![
            key(Hue::Blue, format!("User {}%", model::percent(s.user))),
            key(Hue::Violet, format!("System {}%", model::percent(s.kernel))),
        ],
    );
    let memory = card(
        "Memory",
        match strain {
            Some(w) => format!("{} of {} · {w}", model::bytes(mem.used()), model::bytes(mem.total)),
            None => format!("{} of {}", model::bytes(mem.used()), model::bytes(mem.total)),
        },
        graph(
            [h.mem_used.clone(), h.mem_cached.clone()],
            [Hue::Cyan, Hue::Blue],
            Style::Over,
            Scale::Fixed(1.0),
            "Memory use over two minutes".into(),
        ),
        vec![
            key(Hue::Cyan, format!("Used {}", model::bytes(mem.used()))),
            key(Hue::Blue, format!("Cached {}", model::bytes(mem.cached + mem.buffers))),
        ],
    );
    let disk = card(
        "Disk",
        format!("{} · {}", model::rate(s.disk_read), model::rate(s.disk_write)),
        graph(
            [h.disk_read.clone(), h.disk_write.clone()],
            [Hue::Blue, Hue::Violet],
            Style::Mirrored,
            Scale::Auto,
            "Storage read and written over two minutes".into(),
        ),
        vec![
            key(Hue::Blue, format!("Read {}", model::rate(s.disk_read))),
            key(Hue::Violet, format!("Written {}", model::rate(s.disk_write))),
        ],
    );
    let net = card(
        "Network",
        format!("{} · {}", model::rate(s.net_rx), model::rate(s.net_tx)),
        graph(
            [h.net_rx.clone(), h.net_tx.clone()],
            [Hue::Green, Hue::Amber],
            Style::Mirrored,
            Scale::Auto,
            "Network received and sent over two minutes".into(),
        ),
        vec![
            key(Hue::Green, format!("In {}", model::rate(s.net_rx))),
            key(Hue::Amber, format!("Out {}", model::rate(s.net_tx))),
        ],
    );
    // The GPU lens puts each card's graph in the place of Network's, then
    // Disk's and on: the processor's and memory's keep theirs.
    let mut cards: Vec<Element<Msg>> = [cpu, memory, disk, net].into_iter().map(|c| slot(c).into()).collect();
    let gpus = &m.frame.gpus;
    let order = model::gpu_order(gpus);
    if m.lens == Lens::Gpu && !order.is_empty() {
        let names = model::gpu_names(gpus);
        cards.truncate(4usize.saturating_sub(order.len()));
        cards.extend(order.iter().map(|&i| super::gpu::strip_card(m, i, &names[i])));
    }
    let many = cards.len() > 4;
    let strip = aizome_kit::widgets::layout::row(cards).spacing(2.0).width(Length::Fill);
    // More cards than four scroll sideways, each at its graph's own width.
    let strip: Element<Msg> = if many { scroll(strip).horizontal().width(Length::Fill).into() } else { strip.into() };
    container(strip)
        .padding([5.0, 7.0, 5.0, 7.0])
        .fill(Tone::Chrome)
        .edges(Edge::BevelLo, Sides::TOP)
        .width(Length::Fill)
        .id("graphs")
        .into()
}

fn status(m: &Myaku) -> Element<Msg> {
    let f: &Arc<Frame> = &m.frame;
    let s = &f.system;
    let (left, error) = match (&m.notice, m.gpu_filter()) {
        (Some((n, e)), _) => (n.clone(), *e),
        (None, _) if m.frames == 0 => (String::new(), false),
        (None, Some(device)) => (super::gpu::clients(m, device), false),
        (None, None) => (
            format!(
                "{} · {} · up {}",
                model::count(s.processes, "process", "processes"),
                model::count(s.threads as usize, "thread", "threads"),
                model::uptime(s.uptime)
            ),
            false,
        ),
    };
    let right = match m.lens {
        Lens::Cpu => {
            let mut parts = Vec::new();
            if !s.cpu_name.is_empty() {
                parts.push(s.cpu_name.clone());
            }
            if !s.cores.is_empty() {
                parts.push(model::count(s.cores.len(), "core", "cores"));
            }
            if let (Some(lo), Some(hi)) =
                (s.frequencies.iter().copied().reduce(f64::min), s.frequencies.iter().copied().reduce(f64::max))
            {
                parts.push(if (hi - lo) < 50.0 {
                    model::frequency(hi)
                } else {
                    format!("{}–{}", model::frequency(lo), model::frequency(hi))
                });
            }
            if let Some(t) = s.temperature {
                parts.push(format!("{t:.0} °C"));
            }
            parts.push(format!("load {:.2} {:.2} {:.2}", s.load[0], s.load[1], s.load[2]));
            parts.join(" · ")
        }
        Lens::Memory => format!("Swap {} of {}", model::bytes(s.memory.swap_used), model::bytes(s.memory.swap_total)),
        Lens::Energy => {
            let watts = |w: Option<f64>| w.map(model::watts).unwrap_or_else(|| "—".into());
            let mut parts = match f.power.meter {
                Meter::Rapl | Meter::Package => vec![format!("Processor {}", watts(f.power.cpu))],
                Meter::Battery => vec![format!("Battery {}", watts(f.power.battery))],
                Meter::Graphics => Vec::new(),
                Meter::None => vec!["No power meter".into()],
            };
            if f.power.graphics.is_some() {
                parts.push(format!("Graphics {}", watts(f.power.graphics)));
            }
            parts.join(" · ")
        }
        Lens::Gpu => super::gpu::status(m),
        Lens::Disk => s.disks.iter().map(|d| format!("{} {:.0}% busy", d.name, d.busy)).collect::<Vec<_>>().join(" · "),
    };
    let right = aizome_kit::status_text(right);
    let right = if m.lens == Lens::Energy {
        mouse_area(right)
            .on_press(Msg::EnergyMeters)
            .cursor(aizome_kit::sidestep_ui::Cursor::Pointer)
            .tooltip("Show the energy meters Myaku is using")
            .label("Energy meters")
            .id("energy-meters")
            .into()
    } else {
        right
    };
    aizome_kit::status_bar(vec![
        text(left).size(11.5).role(if error { Role::Danger } else { Role::Text3 }).width(Length::Fill).into(),
        right,
    ])
}
