//! The GPU lens's own parts. Each graphics card has a graph in the strip
//! along the bottom, in the place of Disk's and Network's: how busy it is
//! and how full its memory, over two minutes. With several cards, pressing
//! one shows its clients alone, as the toolbar's pop-up and View ▸ GPUs do;
//! the card shown is framed in the focus colour.

use aizome_kit::prelude::*;
use aizome_kit::sidestep_ui::Cursor;

use super::graph::{Hue, Scale, Style, graph};
use super::view::{card, key, slot};
use crate::app::{Msg, Myaku};
use crate::history::Ring;
use crate::model::{self, Frame, Gpu, Process};

/// Card `i`'s graph in the strip, `name` its label.
pub fn strip_card(m: &Myaku, i: usize, name: &str) -> Element<Msg> {
    let g = &m.frame.gpus[i];
    let history = m.history.card(&g.pci);
    // A figure not read now isn't drawn as if it were naught.
    let series = |ring: Option<&Ring>, known: bool| ring.filter(|_| known).cloned().unwrap_or_default();
    let share = g.vram_used.zip(g.vram_total).filter(|(_, total)| *total > 0);
    let busy = g.busy.map_or("—".into(), |b| format!("{}%", model::percent(b)));
    let mut keys = vec![key(Hue::Violet, format!("Busy {busy}"))];
    // Integrated Intel cards have no memory of their own to tell of.
    let has_memory = g.vram_used.is_some() || g.vram_total.is_some();
    if has_memory {
        let percent = share.map_or("—".into(), |(used, total)| format!("{:.0}%", used as f64 / total as f64 * 100.0));
        keys.push(key(Hue::Cyan, format!("Memory {percent}")));
    }
    let value = match (g.vram_used, g.vram_total) {
        (Some(used), Some(total)) => format!("{} of {}", model::bytes(used), model::bytes(total)),
        (Some(used), None) => model::bytes(used),
        (None, Some(total)) => format!("— of {}", model::bytes(total)),
        (None, None) => busy,
    };
    let body = card(
        name,
        value,
        graph(
            [series(history.map(|h| &h.busy), g.busy.is_some()), series(history.map(|h| &h.memory), share.is_some())],
            [Hue::Violet, Hue::Cyan],
            Style::Overlaid,
            Scale::Fixed(1.0),
            format!("{name}: how busy, and how full its memory, over two minutes"),
        ),
        keys,
    );
    let several = m.frame.gpus.len() > 1;
    let shown = several && m.gpu.as_deref() == Some(g.pci.as_str());
    let mut framed = slot(body);
    if shown {
        framed = framed.border(Edge::Focus);
    } else if several && m.hover_gpu.as_deref() == Some(g.pci.as_str()) {
        framed = framed.border(Edge::Line2);
    }
    let mut about = vec![g.name.clone(), describe(g)];
    if !several {
        return framed.tooltip(about.join("\n")).key(g.pci.clone()).id(format!("gpu-card-{i}")).into();
    }
    about.push(if shown { "Show every GPU's processes".into() } else { "Show only the processes using it".into() });
    let pci = g.pci.clone();
    mouse_area(framed)
        .on_press(Msg::Gpu((!shown).then(|| pci.clone())))
        .on_hover(move |inside| Msg::HoverGpu(pci.clone(), inside))
        .cursor(Cursor::Pointer)
        .tooltip(about.join("\n"))
        .label(if shown { format!("{name}, shown alone") } else { format!("Show {name} alone") })
        .key(g.pci.clone())
        .id(format!("gpu-card-{i}"))
        .into()
}

/// A card's kind, driver, address and figures: "Discrete · nvidia ·
/// 0000:01:00.0 · 2.61 GHz · 52 °C · 180 W".
fn describe(g: &Gpu) -> String {
    let mut parts = vec![if g.integrated { "Integrated" } else { "Discrete" }.to_owned()];
    parts.extend([g.driver.clone(), g.pci.clone()].into_iter().filter(|s| !s.is_empty()));
    parts.extend(figures(g, true));
    parts.join(" · ")
}

/// What a card measures of itself: its temperature and power, and with
/// `clock`, its clock first.
fn figures(g: &Gpu, clock: bool) -> Vec<String> {
    let mut v = Vec::new();
    if let Some(mhz) = g.clock.filter(|c| clock && *c > 0.0) {
        v.push(model::frequency(mhz));
    }
    if let Some(t) = g.temperature {
        v.push(format!("{t:.0} °C"));
    }
    if let Some(w) = g.power {
        v.push(model::watts(w));
    }
    v
}

/// The status line's right in the GPU lens: the card shown, in full, with
/// its figures ("NVIDIA GeForce RTX 5090 · 2.61 GHz · 62 °C · 461 W"); with
/// every card shown, each card's, as the Disk lens gives each disk's.
pub fn status(m: &Myaku) -> String {
    let gpus = &m.frame.gpus;
    match m.gpu_filter().or((gpus.len() == 1).then_some(0)) {
        Some(i) => std::iter::once(gpus[i].name.clone()).chain(figures(&gpus[i], true)).collect::<Vec<_>>().join(" · "),
        None => {
            let names = model::gpu_names(gpus);
            model::gpu_order(gpus)
                .into_iter()
                .map(|i| {
                    std::iter::once(names[i].clone()).chain(figures(&gpus[i], false)).collect::<Vec<_>>().join(" ")
                })
                .collect::<Vec<_>>()
                .join(" · ")
        }
    }
}

/// The status line's left with one card shown: how many processes use it.
pub fn clients(m: &Myaku, device: usize) -> String {
    let f = &m.frame;
    let n = f.processes.iter().filter(|p| p.uses(device)).count();
    let name = &model::gpu_names(&f.gpus)[device];
    match n {
        0 => format!("No process uses {name}"),
        1 => format!("1 of {} processes uses {name}", f.processes.len()),
        _ => format!("{n} of {} processes use {name}", f.processes.len()),
    }
}

/// What the table says with one card shown and no rows.
pub fn nothing(f: &Frame, device: usize) -> String {
    let name = &model::gpu_names(&f.gpus)[device];
    if f.gpus[device].clients_unknown() {
        format!("The processes using {name} can't be read here.")
    } else {
        format!("Nothing is using {name}.")
    }
}

/// What the Card column says of processes: the cards they use, by `names`.
pub fn used_by<'a>(procs: impl Iterator<Item = &'a Process> + Clone, names: &[String]) -> Option<String> {
    let used: Vec<usize> = (0..names.len()).filter(|&d| procs.clone().any(|p| p.uses(d))).collect();
    match used.len() {
        0 => None,
        1 => Some(names[used[0]].clone()),
        2 if names.len() == 2 => Some("Both".into()),
        n => Some(format!("{n} GPUs")),
    }
}
