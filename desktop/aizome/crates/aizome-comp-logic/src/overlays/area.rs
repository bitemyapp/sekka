//! Picking what to screenshot (❖⇧4): drag a rubber band over an area, or
//! press Space and click a window; Esc cancels. The shell and the share
//! picker borrow it (IPC `pick-area`): then the pick is reported as an
//! `area-picked` event instead, a click picks the whole display under the
//! pointer, what may be picked can be narrowed (a share is a display or a
//! window, never an area), and a banner across the displays can say what
//! the pick is for.

use smithay::desktop::layer_map_for_output;
use smithay::input::keyboard::{Keysym, keysyms};
use smithay::input::pointer::CursorIcon;
use smithay::output::Output;

use crate::decor::DecorCache;
use crate::prelude::*;
use crate::wm::{Rect, WinId};
use aizome_comp_core::logic::KeyDecision;
use aizome_comp_core::painter::{Box2, Painter, bold, mono, mono_bold};

pub struct AreaPick {
    pub start: Option<(f64, f64)>,
    pub pos: (f64, f64),
    /// Space: pick a window instead.
    pub window_mode: bool,
    /// Report the pick (IPC `pick-area`) rather than take a screenshot.
    pub report: Option<Report>,
    pub cache: DecorCache,
}

/// A pick to report rather than screenshot.
pub struct Report {
    pub tag: String,
    /// The banner's words.
    pub prompt: Option<String>,
    /// What may be picked: an area dragged over, a display (a click), a
    /// window (Space, then a click).
    pub area: bool,
    pub display: bool,
    pub window: bool,
}

impl Report {
    pub fn new(tag: String, prompt: Option<String>, targets: &[aizome_comp_core::aizome_ipc::PickTarget]) -> Report {
        use aizome_comp_core::aizome_ipc::PickTarget;
        let all = targets.is_empty();
        let has = |t| all || targets.contains(&t);
        let (area, display, window) = (has(PickTarget::Area), has(PickTarget::Display), has(PickTarget::Window));
        Report { tag, prompt: prompt.filter(|p| !p.trim().is_empty()), area, display, window }
    }

    /// Only windows: the picker starts at them, and Space stays.
    fn windows_only(&self) -> bool {
        self.window && !self.area && !self.display
    }
}

pub fn start(state: &mut State, report: Option<Report>) {
    // A picker already up for someone else gives up first.
    if let Some(old) = state.lg.m().overlays.area.take()
        && let Some(r) = old.report
    {
        emit(state, r.tag, None, None);
    }
    if let Some(r) = &report {
        state.az.ipc.emit(&aizome_comp_core::aizome_ipc::Event::PickStarted { tag: r.tag.clone() });
    }
    let pos = state.az.pointer_pos;
    let window_mode = report.as_ref().is_some_and(Report::windows_only);
    state.lg.m().overlays.area =
        Some(AreaPick { start: None, pos: (pos.x, pos.y), window_mode, report, cache: DecorCache::default() });
    state.az.cursor_override = Some(if window_mode { CursorIcon::Pointer } else { CursorIcon::Crosshair });
    state.update_keyboard_focus();
    state.az.queue_redraw_all();
}

/// Closes the picker; returns what to report its pick under, if someone
/// asked for it.
fn finish(state: &mut State) -> Option<Report> {
    let report = state.lg.m().overlays.area.take().and_then(|a| a.report);
    state.az.cursor_override = None;
    state.update_keyboard_focus();
    state.az.queue_redraw_all();
    report
}

/// Tells subscribers what was picked (`None`: nothing, Esc).
fn emit(state: &mut State, tag: String, area: Option<Rect>, window: Option<WinId>) {
    let output = area.and_then(|r| {
        let centre = smithay::utils::Point::from((r.x as f64 + r.w as f64 / 2.0, r.y as f64 + r.h as f64 / 2.0));
        state.az.output_at(centre).map(|o| o.name())
    });
    let area = area.map(|r| aizome_comp_core::aizome_ipc::Rect::new(r.x, r.y, r.w, r.h));
    let toplevel = window.and_then(|w| aizome_comp_core::protocols::foreign_toplevel::identifier(&state.az, w));
    state.az.ipc.emit(&aizome_comp_core::aizome_ipc::Event::AreaPicked { tag, area, output, window, toplevel });
}

/// A window's frame with its tab (what the picker lights up).
fn window_bounds(st: &mut State, id: WinId) -> Option<Rect> {
    let (az, l) = (&mut st.az, st.lg.r());
    l.decor_layout(&mut az.labels, id).map(|d| d.bounds()).or_else(|| l.wm.get(id).map(|w| w.rect))
}

fn band(a: (f64, f64), b: (f64, f64)) -> Rect {
    let x0 = a.0.min(b.0).floor() as i32;
    let y0 = a.1.min(b.1).floor() as i32;
    let x1 = a.0.max(b.0).ceil() as i32;
    let y1 = a.1.max(b.1).ceil() as i32;
    Rect::new(x0, y0, (x1 - x0).max(1), (y1 - y0).max(1))
}

/// The window under the pointer, for window mode.
fn window_at(state: &mut State, x: f64, y: f64) -> Option<WinId> {
    match state.under(smithay::utils::Point::from((x, y))) {
        crate::input::Under::Surface(_, _, aizome_comp_core::input::Owner::Window(id))
        | crate::input::Under::Decor(id, _) => Some(id),
        _ => None,
    }
}

pub fn pointer_button(state: &mut State, pressed: bool, x: f64, y: f64) -> bool {
    let Some(a) = state.lg.m().overlays.area.as_mut() else { return false };
    if a.window_mode {
        // A click picks on the press; its release, the picker gone, goes
        // to nobody (`Interactive::overlay_presses`).
        if pressed {
            let reporting = a.report.is_some();
            let w = window_at(state, x, y);
            if w.is_none() && reporting {
                // Nothing there to pick: keep picking.
                return true;
            }
            let bounds = w.and_then(|w| window_bounds(state, w));
            match finish(state) {
                Some(r) => emit(state, r.tag, bounds, w),
                None => {
                    if let Some(w) = w
                        && let Err(e) = aizome_comp_core::screenshot::take(
                            state,
                            aizome_comp_core::screenshot::What::Window(w),
                            None,
                            true,
                        )
                    {
                        tracing::warn!("screenshot: {e}");
                    }
                }
            }
        }
        return true;
    }
    if pressed {
        a.start = Some((x, y));
        a.pos = (x, y);
    } else if let Some(s) = a.start.take() {
        let r = band(s, (x, y));
        let dragged = r.w > 2 && r.h > 2 && a.report.as_ref().is_none_or(|r| r.area);
        let displays = a.report.as_ref().is_none_or(|r| r.display);
        // A click, when reporting, is the whole display under the pointer
        // (and so is a drag when areas aren't wanted).
        let display = state.az.output_at(smithay::utils::Point::from((x, y))).map(|o| state.az.output_rect(&o));
        if !dragged && !displays {
            // Only areas are wanted: a click picks nothing.
            return true;
        }
        match finish(state) {
            Some(report) => emit(state, report.tag, if dragged { Some(r) } else { display }, None),
            None => {
                if dragged
                    && let Err(e) = aizome_comp_core::screenshot::take(
                        state,
                        aizome_comp_core::screenshot::What::Area(r),
                        None,
                        true,
                    )
                {
                    tracing::warn!("screenshot: {e}");
                }
            }
        }
    }
    true
}

pub fn pointer_moved(state: &mut State, x: f64, y: f64) {
    if let Some(a) = state.lg.m().overlays.area.as_mut() {
        a.pos = (x, y);
        state.az.queue_redraw_all();
    }
}

pub fn key(state: &mut State, syms: &[Keysym], pressed: bool) -> Option<KeyDecision> {
    let a = state.lg.m().overlays.area.as_mut()?;
    if pressed {
        if syms.iter().any(|s| s.raw() == keysyms::KEY_Escape) {
            if let Some(r) = finish(state) {
                emit(state, r.tag, None, None);
            }
        } else if syms.iter().any(|s| s.raw() == keysyms::KEY_space) {
            // Between windows and the rest, where both may be picked.
            let toggles = a.report.as_ref().is_none_or(|r| r.window && (r.area || r.display));
            if toggles {
                a.window_mode = !a.window_mode;
                a.start = None;
                state.az.cursor_override =
                    Some(if a.window_mode { CursorIcon::Pointer } else { CursorIcon::Crosshair });
                state.az.queue_redraw_all();
            }
        }
    }
    Some(KeyDecision::Swallow)
}

/// The chip at the pointer: what a click (or Space) does now.
fn chip_label(a: &AreaPick) -> String {
    let Some(r) = &a.report else {
        return if a.window_mode {
            "click a window · Space for an area · Esc".into()
        } else {
            "drag over an area · Space for a window · Esc".into()
        };
    };
    let mut parts: Vec<&str> = Vec::new();
    if a.window_mode {
        parts.push("click a window");
        if r.area {
            parts.push("Space for an area");
        } else if r.display {
            parts.push("Space for a screen");
        }
    } else {
        if r.area {
            parts.push("drag over an area");
        }
        if r.display {
            parts.push("click for the screen");
        }
        if r.window {
            parts.push("Space for a window");
        }
    }
    parts.push("Esc");
    parts.join(" · ")
}

pub fn items(st: &mut State, output: &Output, scale: f64) -> aizome_comp_core::scene::Scene {
    let (az, l) = (&mut st.az, st.lg.m());
    let Some(mut a) = l.overlays.area.take() else { return Vec::new() };
    let geo = az.output_geometry(output);
    let origin = (geo.loc.x as f64, geo.loc.y as f64);
    let mut pt: Painter = Painter::new(origin, scale, 1.0);
    let p = az.palette;
    // A whole display gets only an outline, no wash.
    let mut wash = true;
    let rect = if a.window_mode {
        let w = l.wm.shown_stacking().into_iter().rev().find(|id| {
            l.decor_layout(&mut az.labels, *id)
                .map(|l| l.bounds())
                .or_else(|| l.wm.get(*id).map(|w| w.rect))
                .is_some_and(|r| r.contains(a.pos.0, a.pos.1))
        });
        w.and_then(|id| l.decor_layout(&mut az.labels, id).map(|l| l.bounds()).or_else(|| l.wm.get(id).map(|w| w.rect)))
    } else if let Some(s) = a.start.filter(|_| a.report.as_ref().is_none_or(|r| r.area)) {
        Some(band(s, a.pos))
    } else if a.report.as_ref().is_some_and(|r| r.display) {
        // When reporting, a click takes the display under the pointer:
        // show which.
        wash = false;
        az.output_at(smithay::utils::Point::from(a.pos)).map(|o| az.output_rect(&o))
    } else {
        None
    };
    let label = match (a.start, a.window_mode, rect) {
        (Some(_), false, Some(r)) if wash => format!("{} × {}", r.w, r.h),
        _ => chip_label(&a),
    };
    if let Some(r) = rect {
        let b = Box2::new(r.x as f64, r.y as f64, r.w as f64, r.h as f64);
        if wash {
            pt.fill(b, p.focus.with_alpha(0.15));
        }
        pt.outline(b, 1.0, p.focus);
    }
    if let Some(r) = &a.report
        && let Some(prompt) = &r.prompt
    {
        let shadows = az.settings.appearance.effects.shadows();
        banner(&mut pt, &mut az.labels, &p, shadows, output, geo, prompt, &banner_hints(r));
    }
    let lb = Box2::new(a.pos.0 + 14.0, a.pos.1 + 14.0, 10.0, 20.0);
    if geo.to_f64().contains(smithay::utils::Point::from(a.pos)) {
        let (tw, _) = az.labels.measure(&label, aizome_comp_core::text::Face::Mono, 11.0);
        let chip = Box2::new(lb.x, lb.y, tw.ceil() + 12.0, 20.0);
        pt.fill(chip, p.menu_title);
        pt.text_center(&mut az.labels, &label, mono(11.0, p.on_menu_title), chip);
    }
    let out = pt.finish(&mut a.cache);
    l.overlays.area = Some(a);
    out
}

/// The keys under the banner's words: what picks what.
fn banner_hints(r: &Report) -> Vec<(&'static str, &'static str)> {
    let mut hints = Vec::new();
    if r.area {
        hints.push(("drag", "an area"));
    }
    if r.display {
        hints.push(("click", "a screen"));
    }
    if r.window {
        hints.push(if r.windows_only() { ("click", "a window") } else { ("Space", "then click, a window") });
    }
    hints.push(("Esc", "cancel"));
    hints
}

/// The banner saying what the pick is for: a raised panel at the top of
/// the display's usable area, the accent (the sharing indicator's blue)
/// down its left edge, the words in bold and the keys beneath, set as the
/// overview's hints are.
#[allow(clippy::too_many_arguments)]
fn banner(
    pt: &mut Painter,
    labels: &mut aizome_comp_core::text::Labels,
    p: &aizome_comp_core::aizome_theme::Palette,
    shadows: bool,
    output: &Output,
    geo: smithay::utils::Rectangle<i32, smithay::utils::Logical>,
    prompt: &str,
    hints: &[(&str, &str)],
) {
    const PAD: f64 = 16.0;
    const KEY_GAP: f64 = 6.0;
    const HINT_GAP: f64 = 22.0;
    let usable = layer_map_for_output(output).non_exclusive_zone();
    let title = bold(14.0, p.text);
    let (tw, _) = labels.measure(prompt, title.face, title.size());
    let mut hw = 0.0;
    for (k, what) in hints {
        hw += labels.measure(k, aizome_comp_core::text::Face::MonoBold, 11.0).0.ceil() + KEY_GAP;
        hw += labels.measure(what, aizome_comp_core::text::Face::Mono, 11.0).0.ceil() + HINT_GAP;
    }
    hw -= HINT_GAP;
    let w = (tw.ceil().max(hw) + 2.0 * PAD + 4.0).min(geo.size.w as f64 - 32.0);
    let h = 64.0;
    let x = geo.loc.x as f64 + usable.loc.x as f64 + ((usable.size.w as f64 - w) / 2.0).round();
    let y = geo.loc.y as f64 + usable.loc.y as f64 + 16.0;
    pt.panel(Box2::new(x, y, w, h), p.raised, p, shadows);
    pt.fill(Box2::new(x + 1.0, y + 1.0, 4.0, h - 2.0), p.blue);
    let left = x + PAD + 4.0;
    let max = w - 2.0 * PAD - 4.0;
    pt.text_mid(labels, prompt, title, left, y + 8.0, 26.0, max);
    let mut hx = left;
    let hy = y + 36.0;
    for (k, what) in hints {
        hx += pt.text_mid(labels, k, mono_bold(11.0, p.text2), hx, hy, 20.0, 80.0) + KEY_GAP;
        hx += pt.text_mid(labels, what, mono(11.0, p.text3), hx, hy, 20.0, 300.0) + HINT_GAP;
    }
}
