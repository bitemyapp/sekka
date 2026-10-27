//! The recovery overlay's look: an opaque backdrop over every output (in
//! the desktop's colour, as a secure attention screen: nothing below is
//! composited, so it costs the same over a stuck game as over an empty
//! desk, and opening it is one cheap frame) and, on one, a panel in
//! Aizome's look (square, `raised`, 1pt bevels, hard 3pt
//! shadow) with a `menu-title` strip, the apps as rows (the selected one
//! on `selection` with a 2pt `focus` rail, as the palette's), the app
//! actions under them, the system actions, and key hints.
//!
//! The geometry is fixed (no measuring), so drawing and hit testing agree
//! and opening it measures nothing. Its labels are its own cache, warmed
//! ahead (see [`View::warm`]), so a first open rasterizes nothing it knew
//! it would need.

use std::collections::HashMap;

use aizome_theme::{Brand, Palette};
use smithay::output::Output;
use smithay::utils::{Logical, Point};

use super::{Act, Hit, Row, Sel};
use crate::painter::{Box2, Painter, bold, mono, semibold};
use crate::pieces::DecorCache;
use crate::scene::Scene;
use crate::state::{Aizome, State};
use crate::text::{Face, Labels, Style};

/// Rows that show at once; more scroll.
pub const VISIBLE_ROWS: usize = 8;
/// App icons, points.
pub const ICON: i32 = 20;

const W: f64 = 620.0;
const PAD: f64 = 16.0;
const TITLE_H: f64 = 30.0;
const MESSAGE_H: f64 = 34.0;
const HEAD_H: f64 = 26.0;
const ROW_H: f64 = 32.0;
const BUTTON_H: f64 = 28.0;
const GAP: f64 = 8.0;
const FOOTER_H: f64 = 30.0;
/// Right edges of the CPU and memory columns, from the panel's right.
const CPU_RIGHT: f64 = 118.0;
const MEM_RIGHT: f64 = 24.0;
const STATUS_RIGHT: f64 = 170.0;
/// Widths labels are fitted to (fixed, so warmed labels are the ones
/// drawn).
const NAME_MAX_W: f64 = 220.0;
const CAPTION_MAX_W: f64 = 360.0;
const BUTTON_LABEL_MAX_W: f64 = 140.0;
const RIGHT_MAX_W: f64 = 160.0;

#[derive(Default)]
pub struct View {
    labels: Labels,
    caches: HashMap<String, DecorCache>,
    /// Scales (in quarters) the fixed labels are warm for.
    warm: Vec<(u32, Palette)>,
}

/// Where everything goes on an output.
pub struct Layout {
    pub panel: Box2,
    pub title: Box2,
    pub message: Option<Box2>,
    pub head: Option<Box2>,
    pub list: Option<Box2>,
    pub rows: Vec<(String, Box2)>,
    pub acts: Vec<(Act, Box2)>,
    pub app_caption: Option<(f64, f64)>,
    pub system_head: Box2,
    pub footer: Box2,
}

/// The actions in rows as they are laid out: the app's pair, then the
/// system's three to a row.
pub fn layout_rows(acts: &[Act]) -> Vec<Vec<Act>> {
    let mut out = Vec::new();
    let app: Vec<Act> = acts.iter().copied().filter(|a| a.per_app()).collect();
    if !app.is_empty() {
        out.push(app);
    }
    let system: Vec<Act> = acts.iter().copied().filter(|a| !a.per_app()).collect();
    for chunk in system.chunks(3) {
        out.push(chunk.to_vec());
    }
    out
}

pub fn layout(az: &Aizome, output: &Output) -> Layout {
    let r = &az.recovery;
    let locked = az.lock.is_locked();
    let geo = az.output_geometry(output);
    let (gx, gy, gw, gh) = (geo.loc.x as f64, geo.loc.y as f64, geo.size.w as f64, geo.size.h as f64);
    let w = W.min(gw - 32.0).max(320.0);
    let shown_rows = r.rows.len().clamp(1, VISIBLE_ROWS) as f64;
    let acts = r.actions(locked);
    let rows_of_acts = layout_rows(&acts);
    let system_rows = rows_of_acts.iter().filter(|r| !r[0].per_app()).count() as f64;
    let message = r.message.is_some() || locked;
    let message_h = if r.message.as_deref().is_some_and(|m| m.contains('\n')) { MESSAGE_H + 16.0 } else { MESSAGE_H };
    let mut h = TITLE_H + if message { message_h } else { 0.0 };
    if !locked {
        h += HEAD_H + shown_rows * ROW_H + GAP + BUTTON_H + GAP + 4.0;
    }
    h += HEAD_H + system_rows * (BUTTON_H + GAP) + 4.0 + FOOTER_H;
    let x0 = (gx + (gw - w) / 2.0).round();
    let y0 = (gy + ((gh - h) / 2.5).max(16.0)).round();
    let panel = Box2::new(x0, y0, w, h);
    let mut y = y0;
    let title = Box2::new(x0, y, w, TITLE_H);
    y += TITLE_H;
    let message_box = message.then(|| {
        let b = Box2::new(x0 + 1.0, y, w - 2.0, message_h);
        y += message_h;
        b
    });
    let mut rows = Vec::new();
    let mut acts_out = Vec::new();
    let (mut head, mut list, mut app_caption) = (None, None, None);
    if !locked {
        head = Some(Box2::new(x0 + PAD, y, w - 2.0 * PAD, HEAD_H));
        y += HEAD_H;
        list = Some(Box2::new(x0 + 1.0, y, w - 2.0, shown_rows * ROW_H));
        for (i, row) in r.rows.iter().enumerate().skip(r.scroll).take(VISIBLE_ROWS) {
            let _ = i;
            rows.push((row.app_id.clone(), Box2::new(x0 + 1.0, y, w - 2.0, ROW_H)));
            y += ROW_H;
        }
        y = list.unwrap().bottom() + GAP;
        let mut x = x0 + PAD;
        for a in Act::APP {
            let bw = if a == Act::ForceQuit { 150.0 } else { 120.0 };
            acts_out.push((a, Box2::new(x, y, bw, BUTTON_H)));
            x += bw + GAP;
        }
        app_caption = Some((x + 4.0, y));
        y += BUTTON_H + GAP + 4.0;
    }
    let system_head = Box2::new(x0 + PAD, y, w - 2.0 * PAD, HEAD_H);
    y += HEAD_H;
    let bw = ((w - 2.0 * PAD - 2.0 * GAP) / 3.0).floor();
    for row in rows_of_acts.iter().filter(|r| !r[0].per_app()) {
        let mut x = x0 + PAD;
        for a in row {
            acts_out.push((*a, Box2::new(x, y, bw, BUTTON_H)));
            x += bw + GAP;
        }
        y += BUTTON_H + GAP;
    }
    y += 4.0;
    let footer = Box2::new(x0, y, w, FOOTER_H);
    Layout { panel, title, message: message_box, head, list, rows, acts: acts_out, app_caption, system_head, footer }
}

/// What's under a point, on the panel's output; `None` outside the panel.
pub fn hit(state: &State, pos: Point<f64, Logical>) -> Option<Hit> {
    let az = &state.az;
    let output = az.recovery.output.clone().or_else(|| az.outputs.first().cloned())?;
    let l = layout(az, &output);
    if !l.panel.contains(pos.x, pos.y) {
        return None;
    }
    if let Some((a, _)) = l.rows.iter().find(|(_, b)| b.contains(pos.x, pos.y)) {
        return Some(Hit::App(a.clone()));
    }
    if let Some((a, _)) = l.acts.iter().find(|(_, b)| b.contains(pos.x, pos.y)) {
        return Some(Hit::Act(*a));
    }
    Some(Hit::Panel)
}

impl View {
    /// Rasterizes the fixed labels for these scales ahead of a first open,
    /// and the apps' names.
    pub fn warm(&mut self, palette: &Palette, scales: &[f64], names: &[String]) {
        for s in scales {
            let key = (s.ceil().max(1.0) as u32 * 4, *palette);
            if !self.warm.contains(&key) {
                self.warm.push(key);
                for (text, style, max_w) in fixed_labels(palette) {
                    self.warm_one(text, style, *s, max_w);
                }
            }
            for name in names {
                self.warm_one(name, semibold(13.0, palette.text), *s, NAME_MAX_W);
                self.warm_one(name, semibold(13.0, palette.on_selection), *s, NAME_MAX_W);
                self.warm_one(&format!("for {name}"), semibold(12.5, palette.text3), *s, CAPTION_MAX_W);
            }
        }
    }

    fn warm_one(&mut self, text: &str, style: Style, scale: f64, max_w: f64) {
        let fitted = self.labels.fit(text, style.face, style.size(), max_w);
        let _ = self.labels.get(&fitted, style, scale, max_w + 1.0);
    }
}

/// The labels every opening draws, with their styles and widths, as
/// `draw` draws them.
fn fixed_labels(p: &Palette) -> Vec<(&'static str, Style, f64)> {
    let mut v = vec![
        ("Recovery", bold(13.0, p.on_menu_title), 200.0),
        ("⌃⌥Del or esc closes", mono(11.0, p.on_menu_title), 220.0),
        ("APPS", mono(10.5, p.text3), 120.0),
        ("CPU", mono(10.5, p.text3), RIGHT_MAX_W),
        ("MEMORY", mono(10.5, p.text3), RIGHT_MAX_W),
        ("Really Log Out?", semibold(12.5, p.on_selection), BUTTON_LABEL_MAX_W),
        ("–", mono(12.0, p.text), RIGHT_MAX_W),
        ("–", mono(12.0, p.on_selection), RIGHT_MAX_W),
        ("SYSTEM", mono(10.5, p.text3), 120.0),
        ("No apps are open", semibold(13.0, p.text3), 300.0),
        ("NOT RESPONDING", mono(10.5, p.red), 160.0),
        ("CRASHED", mono(10.5, p.red), 160.0),
        ("STOPPED", mono(10.5, p.amber), 160.0),
        ("NOT RESPONDING", mono(10.5, p.on_selection), 160.0),
        ("CRASHED", mono(10.5, p.on_selection), 160.0),
        ("STOPPED", mono(10.5, p.on_selection), 160.0),
    ];
    for n in ["1 window", "2 windows", "3 windows"] {
        v.push((n, mono(11.0, p.text3), 120.0));
        v.push((n, mono(11.0, p.on_selection), 120.0));
    }
    for a in [Act::APP.as_slice(), Act::SYSTEM.as_slice()].concat() {
        v.push((a.label(false), semibold(12.5, p.text), BUTTON_LABEL_MAX_W));
        v.push((a.label(false), semibold(12.5, p.on_selection), BUTTON_LABEL_MAX_W));
        v.push((a.label(false), semibold(12.5, p.faint), BUTTON_LABEL_MAX_W));
        v.push((a.key_cap(), mono(11.0, p.faint), 40.0));
        v.push((a.key_cap(), mono(11.0, p.text3), 40.0));
        v.push((a.key_cap(), mono(11.0, p.on_selection), 40.0));
    }
    v.push((Act::ForceQuit.label(false), semibold(12.5, p.red), BUTTON_LABEL_MAX_W));
    v.push((Act::RestartShell.label(true), semibold(12.5, p.text), BUTTON_LABEL_MAX_W));
    v.push((Act::RestartShell.label(true), semibold(12.5, p.on_selection), BUTTON_LABEL_MAX_W));
    for (cap, what) in FOOTER {
        v.push((cap, mono(11.0, p.text2), 60.0));
        v.push((what, mono(11.0, p.text3), 160.0));
    }
    v
}

const FOOTER: [(&str, &str); 4] = [("↑↓←→", "move"), ("↩", "choose"), ("Del", "force quit"), ("esc", "close")];

/// The scrim on every output; the panel on its own.
pub fn items(az: &mut Aizome, output: &Output, scale: f64) -> Scene {
    if !az.recovery.open {
        return Vec::new();
    }
    let Some(mut view) = az.recovery.view.take() else { return Vec::new() };
    let started = std::time::Instant::now();
    let labels_before = view.labels.len();
    std::mem::swap(&mut az.labels, &mut view.labels);
    let out = draw(az, &mut view, output, scale);
    std::mem::swap(&mut az.labels, &mut view.labels);
    let new_labels = view.labels.len() - labels_before;
    az.recovery.view = Some(view);
    if let Some(t) = az.recovery.opened_at.take() {
        tracing::debug!(since_open = ?t.elapsed(), built_in = ?started.elapsed(), new_labels, "recovery overlay's first frame");
    }
    out
}

fn draw(az: &mut Aizome, view: &mut View, output: &Output, scale: f64) -> Scene {
    let geo = az.output_geometry(output);
    let origin = (geo.loc.x as f64, geo.loc.y as f64);
    let p = az.palette;
    let mut pt = Painter::new(origin, scale, 1.0);
    // Nothing behind shows (or costs anything to draw).
    pt.fill(Box2::new(origin.0, origin.1, geo.size.w as f64, geo.size.h as f64), p.desktop);
    let on_panel = az.recovery.output.as_ref().is_none_or(|o| o == output);
    if on_panel {
        panel(az, &mut pt, output, &p);
    }
    let cache = view.caches.entry(output.name()).or_default();
    pt.finish(cache)
}

#[allow(clippy::too_many_lines)]
fn panel(az: &mut Aizome, pt: &mut Painter, output: &Output, p: &Palette) {
    let l = layout(az, output);
    let locked = az.lock.is_locked();
    let shadows = az.settings.appearance.effects.shadows();
    pt.panel(l.panel, p.raised, p, shadows);

    // The title strip: the mark, "Recovery", how to close.
    pt.fill(Box2::new(l.title.x + 1.0, l.title.y + 1.0, l.title.w - 2.0, l.title.h - 1.0), p.menu_title);
    let brand = Brand::for_theme(p.name);
    let (mx, my, m) = (l.title.x + 12.0, l.title.y + 9.0, 3.0);
    for (r, c) in brand.mark_rects() {
        pt.fill(Box2::new(mx + r[0] as f64 * m, my + r[1] as f64 * m, r[2] as f64 * m, r[3] as f64 * m), c);
    }
    pt.text_mid(&mut az.labels, "Recovery", bold(13.0, p.on_menu_title), mx + 22.0, l.title.y, l.title.h, 200.0);
    let hint = "⌃⌥Del or esc closes";
    let hw = az.labels.measure(hint, Face::Mono, 11.0).0.min(220.0);
    pt.text_mid(
        &mut az.labels,
        hint,
        mono(11.0, p.on_menu_title),
        l.title.right() - 14.0 - hw,
        l.title.y,
        l.title.h,
        220.0,
    );

    // A message: the last action's result, or why so little is offered.
    if let Some(b) = l.message {
        pt.fill(b, p.sunk);
        pt.fill(Box2::new(b.x, b.bottom() - 1.0, b.w, 1.0), p.line);
        let text = az
            .recovery
            .message
            .clone()
            .unwrap_or_else(|| "The screen is locked: only what keeps it locked is offered.".to_string());
        let ink = if text.starts_with("Couldn't") { p.red } else { p.text2 };
        let (first, second) = text.split_once('\n').unwrap_or((text.as_str(), ""));
        if second.is_empty() {
            pt.text_mid(&mut az.labels, first, semibold(12.5, ink), b.x + PAD - 1.0, b.y, b.h, b.w - 2.0 * PAD);
        } else {
            let half = (b.h - 8.0) / 2.0;
            pt.text_mid(&mut az.labels, first, semibold(12.5, ink), b.x + PAD - 1.0, b.y + 4.0, half, b.w - 2.0 * PAD);
            pt.text_mid(
                &mut az.labels,
                second,
                mono(11.0, p.text3),
                b.x + PAD - 1.0,
                b.y + 4.0 + half,
                half,
                b.w - 2.0 * PAD,
            );
        }
    }

    if let (Some(head), Some(list)) = (l.head, l.list) {
        let cap = mono(10.5, p.text3);
        pt.text_mid(&mut az.labels, "APPS", cap, head.x, head.y + 4.0, head.h - 4.0, 120.0);
        right_text(az, pt, "CPU", cap, l.panel.right() - CPU_RIGHT, head.y + 4.0, head.h - 4.0);
        right_text(az, pt, "MEMORY", cap, l.panel.right() - MEM_RIGHT, head.y + 4.0, head.h - 4.0);
        pt.fill(Box2::new(list.x, list.y - 1.0, list.w, 1.0), p.line);
        if az.recovery.rows.is_empty() {
            pt.text_mid(
                &mut az.labels,
                "No apps are open",
                semibold(13.0, p.text3),
                list.x + PAD,
                list.y,
                ROW_H,
                300.0,
            );
        }
        let rows: Vec<(Row, Box2)> = l
            .rows
            .iter()
            .filter_map(|(a, b)| az.recovery.rows.iter().find(|r| &r.app_id == a).map(|r| (r.clone(), *b)))
            .collect();
        for (row, b) in rows {
            app_row(az, pt, &row, b, l.panel, p);
        }
        // More than show: a hint at the bottom edge.
        let r = &az.recovery;
        if r.rows.len() > VISIBLE_ROWS {
            let n = r.rows.len();
            let text = format!("{}–{} of {n}", r.scroll + 1, (r.scroll + VISIBLE_ROWS).min(n));
            right_text(az, pt, &text, mono(10.5, p.text3), l.panel.right() - PAD, head.y + 4.0, head.h - 4.0);
        }
        pt.fill(Box2::new(list.x, list.bottom(), list.w, 1.0), p.line);
    }

    // The actions.
    let sel = az.recovery.sel.clone();
    let hover = az.recovery.hover.clone();
    let app_selected = az.recovery.selected_app().cloned();
    let confirm = az.recovery.confirm.map(|(a, _)| a);
    for (act, b) in l.acts.clone() {
        let enabled = !act.per_app() || app_selected.is_some();
        let focused = sel == Some(Sel::Act(act));
        let hovered = hover == Some(Hit::Act(act));
        let label = if confirm == Some(act) { "Really Log Out?" } else { act.label(locked) };
        button(az, pt, b, label, act.key_cap(), focused, hovered, enabled, act == Act::ForceQuit, p);
    }
    if let (Some((x, y)), Some(row)) = (l.app_caption, app_selected.as_ref()) {
        let max = (l.panel.right() - PAD - x).min(CAPTION_MAX_W);
        let text = format!("for {}", row.name);
        pt.text_mid(&mut az.labels, &text, semibold(12.5, p.text3), x, y, BUTTON_H, max);
    }
    pt.text_mid(
        &mut az.labels,
        "SYSTEM",
        mono(10.5, p.text3),
        l.system_head.x,
        l.system_head.y + 4.0,
        l.system_head.h - 4.0,
        120.0,
    );

    // Key hints.
    let f = l.footer;
    pt.fill(Box2::new(f.x + 1.0, f.y, f.w - 2.0, 1.0), p.line);
    let mut x = f.x + PAD;
    for (cap, what) in FOOTER {
        let cw = pt.text_mid(&mut az.labels, cap, mono(11.0, p.text2), x, f.y, f.h, 60.0);
        let ww = pt.text_mid(&mut az.labels, what, mono(11.0, p.text3), x + cw + 6.0, f.y, f.h, 160.0);
        x += cw + 6.0 + ww + 18.0;
    }
}

#[allow(clippy::too_many_arguments)]
fn app_row(az: &mut Aizome, pt: &mut Painter, row: &Row, b: Box2, panel: Box2, p: &Palette) {
    let selected = az.recovery.sel == Some(Sel::App(row.app_id.clone()));
    let hovered = az.recovery.hover == Some(Hit::App(row.app_id.clone()));
    let (ink, sub) = if selected {
        pt.fill(b, p.selection);
        pt.fill(Box2::new(b.x, b.y, 2.0, b.h), p.focus);
        (p.on_selection, p.on_selection)
    } else {
        // The app Close and Force Quit act on, while one of them has the
        // keys.
        let target = matches!(az.recovery.sel, Some(Sel::Act(a)) if a.per_app())
            && az.recovery.last_app.as_deref() == Some(row.app_id.as_str());
        if target {
            pt.fill(b, p.sunk);
            pt.fill(Box2::new(b.x, b.y, 2.0, b.h), p.focus);
        } else if hovered {
            pt.fill(b, p.sunk);
        }
        (p.text, p.text3)
    };
    let x = b.x + PAD - 1.0;
    pt.icon(&mut az.icons, &row.app_id, crate::icons::Tint::Color, x, b.y + (b.h - ICON as f64) / 2.0, ICON, 1.0);
    let name_x = x + ICON as f64 + 10.0;
    let status_x = panel.right() - STATUS_RIGHT;
    let nw = pt.text_mid(
        &mut az.labels,
        &row.name,
        semibold(13.0, ink),
        name_x,
        b.y,
        b.h,
        (status_x - 180.0 - name_x).clamp(60.0, NAME_MAX_W),
    );
    let n = row.windows.len();
    let count = if n == 1 { "1 window".to_string() } else { format!("{n} windows") };
    pt.text_mid(&mut az.labels, &count, mono(11.0, sub), name_x + nw + 10.0, b.y, b.h, 120.0);
    let status = if row.crashed {
        Some(("CRASHED", p.red))
    } else if row.not_responding {
        Some(("NOT RESPONDING", p.red))
    } else if row.stopped {
        Some(("STOPPED", p.amber))
    } else {
        None
    };
    if let Some((text, c)) = status {
        let c = if selected { p.on_selection } else { c };
        right_text(az, pt, text, mono(10.5, c), status_x, b.y, b.h);
    }
    let sample = az.recovery.samples.get(&row.app_id).cloned();
    let cpu = sample.as_ref().and_then(|s| s.cpu_percent).map(fmt_cpu).unwrap_or_else(|| "–".into());
    let hot = sample.as_ref().and_then(|s| s.cpu_percent).is_some_and(|c| c >= 80.0);
    let cpu_ink = if hot && !selected { p.amber } else { ink };
    right_text(az, pt, &cpu, mono(12.0, cpu_ink), panel.right() - CPU_RIGHT, b.y, b.h);
    let mem = sample.as_ref().and_then(|s| s.memory).map(fmt_bytes).unwrap_or_else(|| "–".into());
    right_text(az, pt, &mem, mono(12.0, ink), panel.right() - MEM_RIGHT, b.y, b.h);
}

#[allow(clippy::too_many_arguments)]
fn button(
    az: &mut Aizome,
    pt: &mut Painter,
    b: Box2,
    label: &str,
    cap: &str,
    focused: bool,
    hovered: bool,
    enabled: bool,
    danger: bool,
    p: &Palette,
) {
    let (ink, sub) = if focused {
        pt.fill(b, p.selection);
        pt.outline(b, 2.0, p.focus);
        (p.on_selection, p.on_selection)
    } else {
        pt.fill(b, p.surface);
        pt.outline(b, 1.0, if hovered && enabled { p.focus } else { p.line2 });
        let ink = if !enabled {
            p.faint
        } else if danger {
            p.red
        } else {
            p.text
        };
        (ink, if enabled { p.text3 } else { p.faint })
    };
    let cw = az.labels.measure(cap, Face::Mono, 11.0).0;
    pt.text_mid(
        &mut az.labels,
        label,
        semibold(12.5, ink),
        b.x + 10.0,
        b.y,
        b.h,
        (b.w - cw - 30.0).clamp(20.0, BUTTON_LABEL_MAX_W),
    );
    pt.text_mid(&mut az.labels, cap, mono(11.0, sub), b.right() - 10.0 - cw, b.y, b.h, 40.0);
}

/// Text with its right edge at `right`.
#[allow(clippy::too_many_arguments)]
fn right_text(az: &mut Aizome, pt: &mut Painter, text: &str, style: Style, right: f64, y: f64, h: f64) {
    let w = az.labels.measure(text, style.face, style.size()).0.ceil();
    pt.text_mid(&mut az.labels, text, style, right - w, y, h, RIGHT_MAX_W);
}

fn fmt_cpu(c: f64) -> String {
    if c < 9.95 { format!("{c:.1}%") } else { format!("{c:.0}%") }
}

pub fn fmt_bytes(b: u64) -> String {
    let b = b as f64;
    let mb = b / (1024.0 * 1024.0);
    if mb >= 1024.0 {
        format!("{:.1} GB", mb / 1024.0)
    } else if mb >= 1.0 {
        format!("{mb:.0} MB")
    } else {
        format!("{:.0} KB", b / 1024.0)
    }
}
