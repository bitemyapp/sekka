//! The Desks overview (⌃❖↑): C4's grid of Desk cards in Aizome's look.
//! Each card shows its Desk's windows where they are, scaled from their
//! current buffers (nothing asks a client to redraw), with a header
//! "N name ~/folder · K windows" and what the Desk is doing: "here", when
//! it was left ("12 min ago"), "parked 3 days · draws nothing", and
//! "● 3 new" for windows asking for attention. The current Desk is
//! outlined in the focus colour; a dashed "+" card adds a Desk.
//!
//! Click a Desk to go there, drag a window's thumbnail to another Desk
//! (a label says where it goes), 1…9 to go to that Desk, p to park the
//! Desk under the pointer (its windows are told they're suspended, so they
//! draw nothing until it's visited) or wake it, ❖-scroll to zoom, type (or
//! / then type) to filter windows by title (Return goes to the first
//! match), Esc to close.

use smithay::input::keyboard::{Keysym, keysyms};
use smithay::output::Output;

use crate::bindings::Mods;
use crate::decor::{DecorCache, TAB_H};
use crate::prelude::*;
use crate::wm::{DeskId, WinId};
use aizome_comp_core::logic::KeyDecision;
use aizome_comp_core::painter::{Box2, Painter, bold, mono, mono_bold};

pub struct Overview {
    pub output: Output,
    pub filter: String,
    pub cache: DecorCache,
    /// From the last drawing: each card (`None` is the "+" card).
    pub cards: Vec<(Option<DeskId>, Box2)>,
    /// Window thumbnails: window, its Desk, its box.
    pub thumbs: Vec<(WinId, DeskId, Box2)>,
    /// A thumbnail being dragged.
    pub drag: Option<ThumbDrag>,
    pub pointer: (f64, f64),
    /// Columns fewer (+) or more (-) than the overview picks by itself.
    pub zoom: i32,
    /// The wheel's movement short of a zoom step.
    pub wheel: f64,
}

/// How long ago, in a word or two: "just now", "12 min", "3 h", "3 days".
pub fn age(secs: u64) -> String {
    match secs {
        0..=59 => "just now".to_string(),
        60..=3599 => format!("{} min", secs / 60),
        3600..=86_399 => format!("{} h", secs / 3600),
        _ => match secs / 86_400 {
            1 => "1 day".to_string(),
            d => format!("{d} days"),
        },
    }
}

/// What a Desk is doing, after its window count: here, parked, or left.
pub fn status(here: bool, parked: Option<u64>, left: Option<u64>, now: u64) -> Option<String> {
    if here {
        return Some("here".to_string());
    }
    if let Some(at) = parked {
        return Some(format!("parked {} · draws nothing", age(now.saturating_sub(at))));
    }
    left.map(|at| match age(now.saturating_sub(at)) {
        a if a == "just now" => a,
        a => format!("{a} ago"),
    })
}

/// A thumbnail being dragged: window, where it was grabbed, where the
/// pointer is.
pub type ThumbDrag = (WinId, (f64, f64), (f64, f64));

pub fn toggle(state: &mut State) {
    if let Some(ov) = state.lg.m().overlays.overview.take() {
        state.az.queue_redraw(&ov.output);
        state.az.window_outputs_dirty = true;
        state.update_keyboard_focus();
        return;
    }
    // The switcher and marking menu give way.
    super::switcher::close(state, false);
    state.lg.m().overlays.marking = None;
    // On the display with the focus, where the keys are.
    let Some(output) = crate::actions::desk_output(state, state.lg.r().wm.current_desk()) else { return };
    state.lg.m().overlays.overview = Some(Overview {
        output: output.clone(),
        filter: String::new(),
        cache: DecorCache::default(),
        cards: Vec::new(),
        thumbs: Vec::new(),
        drag: None,
        pointer: (0.0, 0.0),
        zoom: 0,
        wheel: 0.0,
    });
    let ms = state.az.motion_ms();
    state.lg.m().anims.start_overlay("overview", ms);
    state.update_keyboard_focus();
    state.az.queue_redraw(&output);
}

pub fn close(state: &mut State) {
    if state.lg.m().overlays.overview.is_some() {
        toggle(state);
    }
}

fn matches(l: &Logic, id: WinId, filter: &str) -> bool {
    if filter.is_empty() {
        return true;
    }
    let f = filter.to_lowercase();
    l.wm.get(id).is_some_and(|w| w.title.to_lowercase().contains(&f) || w.app_id.to_lowercase().contains(&f))
}

pub fn key(state: &mut State, syms: &[Keysym], mods: Mods, pressed: bool, utf8: Option<char>) -> Option<KeyDecision> {
    state.lg.m().overlays.overview.as_ref()?;
    if !pressed {
        return Some(KeyDecision::Swallow);
    }
    let is = |k: u32| syms.iter().any(|s| s.raw() == k);
    let filtering = state.lg.m().overlays.overview.as_ref().is_some_and(|o| !o.filter.is_empty());
    if is(keysyms::KEY_Escape) {
        if filtering {
            state.lg.m().overlays.overview.as_mut().unwrap().filter.clear();
        } else {
            close(state);
            return Some(KeyDecision::Swallow);
        }
    } else if is(keysyms::KEY_BackSpace) {
        state.lg.m().overlays.overview.as_mut().unwrap().filter.pop();
    } else if is(keysyms::KEY_Return) || is(keysyms::KEY_KP_Enter) {
        let f = state.lg.m().overlays.overview.as_ref().unwrap().filter.clone();
        let l = state.lg.r();
        let first = l.wm.mru().iter().copied().find(|id| !f.is_empty() && matches(l, *id, &f));
        close(state);
        if let Some(id) = first {
            state.activate_window(id);
        }
        return Some(KeyDecision::Swallow);
    } else if !filtering
        && !mods.ctrl
        && !mods.logo
        && (keysyms::KEY_1..=keysyms::KEY_9).contains(&syms.first().map(|s| s.raw()).unwrap_or(0))
    {
        let n = syms[0].raw() - keysyms::KEY_1 + 1;
        let here = state.lg.r().overlays.overview.as_ref().map(|o| o.output.name());
        close(state);
        if let Some(d) = state.lg.m().wm.desk_at(n) {
            state.show_desk(here.as_deref(), d);
        }
        return Some(KeyDecision::Swallow);
    } else if is(keysyms::KEY_Up) && mods.ctrl && mods.logo {
        close(state);
        return Some(KeyDecision::Swallow);
    } else if !filtering && utf8 == Some('p') && !mods.ctrl && !mods.logo && park_under_pointer(state) {
        // Parked or woken: the card says so.
    } else if !filtering && utf8 == Some('/') {
        // "/" starts a filter (as typing does), and isn't part of it.
    } else if let Some(c) = utf8.filter(|c| !c.is_control() && !mods.ctrl && !mods.logo) {
        state.lg.m().overlays.overview.as_mut().unwrap().filter.push(c);
    }
    let o = state.lg.m().overlays.overview.as_ref().unwrap().output.clone();
    state.az.queue_redraw(&o);
    Some(KeyDecision::Swallow)
}

/// Parks the Desk under the pointer, or wakes it if it's parked. False
/// when the pointer isn't over another Desk's card (then "p" filters).
fn park_under_pointer(state: &mut State) -> bool {
    let Some(ov) = state.lg.m().overlays.overview.as_ref() else { return false };
    let (x, y) = ov.pointer;
    let Some(desk) = ov.cards.iter().find(|(_, b)| b.contains(x, y)).and_then(|(d, _)| *d) else { return false };
    let l = state.lg.r();
    if l.wm.is_visible(desk) {
        return false;
    }
    let parked = l.wm.desk(desk).is_some_and(|d| d.parked.is_some());
    state.set_desk_parked(desk, !parked)
}

/// The wheel over the overview: ❖-scroll (or a plain one) zooms, a step
/// a notch, a touchpad's movement adding up to one.
pub fn scroll(state: &mut State, v: f64) -> bool {
    let Some(ov) = state.lg.m().overlays.overview.as_mut() else { return false };
    ov.wheel += v;
    if ov.wheel.abs() >= 10.0 {
        // Up (away) zooms in: fewer, bigger cards.
        ov.zoom += if ov.wheel < 0.0 { 1 } else { -1 };
        ov.wheel = 0.0;
        let o = ov.output.clone();
        state.az.queue_redraw(&o);
    }
    true
}

pub fn pointer_button(state: &mut State, pressed: bool, x: f64, y: f64) -> bool {
    let Some(ov) = state.lg.m().overlays.overview.as_mut() else { return false };
    if pressed {
        if let Some((id, _, b)) = ov.thumbs.iter().rev().find(|(_, _, b)| b.contains(x, y)) {
            let _ = b;
            ov.drag = Some((*id, (x, y), (x, y)));
            return true;
        }
        let card = ov.cards.iter().find(|(_, b)| b.contains(x, y)).map(|(d, _)| *d);
        let here = ov.output.name();
        match card {
            Some(Some(desk)) => {
                // To this display (another showing it swaps with it).
                close(state);
                state.show_desk(Some(&here), desk);
            }
            Some(None) => {
                let n = state.lg.m().wm.desks().len() + 1;
                state.lg.m().wm.add_desk(n.to_string(), None, None);
                state.emit_desks();
                let o = state.lg.m().overlays.overview.as_ref().unwrap().output.clone();
                state.az.queue_redraw(&o);
            }
            None => {}
        }
        return true;
    }
    // Released: a drag ends on a card, a click on a thumbnail goes to it.
    let Some((id, from, _)) = ov.drag.take() else { return true };
    let moved = (x - from.0).abs() > 4.0 || (y - from.1).abs() > 4.0;
    let target = ov.cards.iter().find(|(_, b)| b.contains(x, y)).map(|(d, _)| *d);
    let o = ov.output.clone();
    if !moved {
        close(state);
        state.activate_window(id);
        return true;
    }
    match target {
        Some(Some(desk)) => state.move_to_desk(id, desk, false),
        Some(None) => {
            let n = state.lg.m().wm.desks().len() + 1;
            let d = state.lg.m().wm.add_desk(n.to_string(), None, None);
            state.move_to_desk(id, d, false);
        }
        None => {}
    }
    state.az.queue_redraw(&o);
    true
}

pub fn pointer_moved(state: &mut State, x: f64, y: f64) {
    let Some(ov) = state.lg.m().overlays.overview.as_mut() else { return };
    ov.pointer = (x, y);
    if let Some(d) = &mut ov.drag {
        d.2 = (x, y);
    }
    let o = ov.output.clone();
    state.az.queue_redraw(&o);
}

pub fn items(st: &mut State, output: &Output, scale: f64) -> aizome_comp_core::scene::Scene {
    let software = st.backend.software();
    let (az, l) = (&mut st.az, st.lg.m());
    let Some(mut ov) = l.overlays.overview.take() else { return Vec::new() };
    if &ov.output != output {
        l.overlays.overview = Some(ov);
        return Vec::new();
    }
    let geo = az.output_geometry(output);
    let g = Box2::new(geo.loc.x as f64, geo.loc.y as f64, geo.size.w as f64, geo.size.h as f64);
    let usable = az.usable_area(output);
    let origin = (g.x, g.y);
    let alpha = l.anims.overlay_progress("overview") as f32;
    let mut pt: Painter = Painter::new(origin, scale, 1.0);
    let p = az.palette;
    // Nothing is drawn under the overview but the frame's clear colour;
    // under pixman, see-through pieces on flat colours are filled as the
    // colours they make rather than blended.
    pt.ground = software.then_some(p.desktop);
    let shadows = az.settings.appearance.effects.shadows();

    // The backdrop covers the desktop.
    pt.fill(g, p.sunk);
    pt.alpha = alpha;

    let desks: Vec<(DeskId, String, Option<String>)> =
        l.wm.desks().iter().map(|d| (d.id, d.name.clone(), d.folder.clone())).collect();
    let total: usize = desks.iter().map(|(d, ..)| l.wm.desk_window_count(*d)).sum();

    // Heading and filter.
    let margin = (g.w * 0.05).clamp(24.0, 64.0);
    let top = g.y + margin * 0.6;
    let hw = pt.text_mid(&mut az.labels, "Desks", bold(20.0, p.text), g.x + margin, top, 32.0, 200.0);
    let fw = 320.0f64.min(g.w * 0.3);
    let field = Box2::new(g.right() - margin - fw, top, fw, 28.0);
    pt.fill(field, p.line2);
    pt.fill(field.inset(1.0), p.surface);
    let (ftext, fcol) = if ov.filter.is_empty() {
        ("type to filter windows".to_string(), p.text3)
    } else {
        (ov.filter.clone(), p.text)
    };
    let sw = pt.text_mid(&mut az.labels, "/", mono_bold(12.0, p.focus), field.x + 10.0, field.y, field.h, 20.0);
    pt.text_mid(&mut az.labels, &ftext, mono(12.0, fcol), field.x + 16.0 + sw, field.y, field.h, field.w - 30.0);

    // The grid: Desks plus the "+" card, at the output's aspect.
    let n = desks.len() + 1;
    let auto = if n <= 2 {
        n
    } else if n <= 4 {
        2
    } else {
        3
    }
    .max(1) as i32;
    // Zoomed: kept to between one column and every card in a row.
    ov.zoom = ov.zoom.clamp(auto - n as i32, auto - 1);
    let cols = (auto - ov.zoom) as usize;
    let rows = n.div_ceil(cols);
    let head_h = 26.0;
    let gap = 32.0;
    let area_top = top + 32.0 + 28.0;
    let area_h = g.bottom() - area_top - 52.0;
    let area_w = g.w - 2.0 * margin;
    let mut cw = (area_w - (cols - 1) as f64 * gap) / cols as f64;
    let mut ch = cw * g.h / g.w;
    let fit_h = (area_h - rows as f64 * head_h - (rows - 1) as f64 * gap) / rows as f64;
    if ch > fit_h {
        ch = fit_h;
        cw = ch * g.w / g.h;
    }
    let grid_w = cols as f64 * cw + (cols - 1) as f64 * gap;
    let gx = g.x + ((g.w - grid_w) / 2.0).round();
    let k = cw / g.w;
    let plural = |n: usize, one: &str, many: &str| if n == 1 { format!("1 {one}") } else { format!("{n} {many}") };
    let summary = format!(
        "{} · {} · zoom {:.0}%",
        plural(desks.len(), "Desk", "Desks"),
        plural(total, "window", "windows"),
        k * 100.0
    );
    pt.text_mid(&mut az.labels, &summary, mono(11.0, p.text3), g.x + margin + hw + 12.0, top + 4.0, 28.0, 360.0);
    let now = crate::actions::now_secs();
    ov.cards.clear();
    ov.thumbs.clear();
    let dragging = ov.drag.map(|d| d.0);
    let current = l.wm.current_desk();
    for i in 0..n {
        let (c, r) = (i % cols, i / cols);
        let x = (gx + c as f64 * (cw + gap)).round();
        let y = (area_top + r as f64 * (ch + head_h + gap) + head_h).round();
        let card = Box2::new(x, y, cw.round(), ch.round());
        let Some((desk, name, folder)) = desks.get(i).cloned() else {
            // "+": a new Desk.
            ov.cards.push((None, card));
            pt.dashed(card, 1.0, p.line2);
            pt.text_center(
                &mut az.labels,
                "+",
                bold(22.0, p.text2),
                Box2::new(card.x, card.y + card.h / 2.0 - 30.0, card.w, 30.0),
            );
            pt.text_center(
                &mut az.labels,
                "new Desk",
                mono(11.0, p.text3),
                Box2::new(card.x, card.y + card.h / 2.0 + 2.0, card.w, 18.0),
            );
            continue;
        };
        ov.cards.push((Some(desk), card));
        // Header: "N name ~/folder · K windows".
        let count = l.wm.desk_window_count(desk);
        let hy = card.y - head_h;
        let mut hx = card.x;
        hx += pt.text_mid(&mut az.labels, &(i + 1).to_string(), mono_bold(12.0, p.focus), hx, hy, head_h - 6.0, 30.0)
            + 8.0;
        // Desks named by their number show it once.
        if name != (i + 1).to_string() {
            hx += pt.text_mid(&mut az.labels, &name, bold(13.0, p.text), hx, hy, head_h - 6.0, card.w * 0.5) + 8.0;
        }
        let mut rest = String::new();
        if let Some(f) = &folder {
            rest.push_str(f);
            rest.push_str(" · ");
        }
        rest.push_str(&if count == 1 { "1 window".to_string() } else { format!("{count} windows") });
        let (parked, left) = l.wm.desk(desk).map_or((None, None), |d| (d.parked, d.left));
        // Another display shows it: which.
        let elsewhere = l.wm.screen_showing(desk).filter(|_| desk != current).map(|s| format!("on {}", s.output));
        if let Some(st) = elsewhere.or_else(|| status(desk == current, parked, left, now)) {
            rest.push_str(" · ");
            rest.push_str(&st);
        }
        if l.desk_audible(desk) {
            rest.push_str(" · audio playing");
        }
        // Windows asking for attention, in amber after the rest.
        let urgent = l.wm.desk_urgent_count(desk);
        let new_w = if urgent > 0 { 70.0 } else { 0.0 };
        hx += pt.text_mid(&mut az.labels, &rest, mono(11.0, p.text3), hx, hy, head_h - 6.0, card.right() - hx - new_w);
        if urgent > 0 {
            let new = format!("● {urgent} new");
            pt.text_mid(&mut az.labels, &new, mono(11.0, p.amber), hx + 8.0, hy, head_h - 6.0, card.right() - hx - 8.0);
        }
        // The card.
        if shadows {
            pt.fill(Box2::new(card.right(), card.y + 3.0, 3.0, card.h), p.slab);
            pt.fill(Box2::new(card.x + 3.0, card.bottom(), card.w - 3.0, 3.0), p.slab);
        }
        pt.fill(card, p.bevel_lo);
        pt.fill(card.inset(1.0), p.desktop);
        // Its windows, bottom to top, where they are on this output (or,
        // laid out on another display, where they would be on this one).
        for id in l.wm.shown_stacking_on(desk) {
            let Some(w) = l.wm.get(id) else { continue };
            let from = w.frame.or_else(|| l.wm.screen_at(w.rect).map(|s| s.usable));
            let r = match from {
                Some(f) if f != usable => crate::wm::scale_frame(w.rect, f, usable, Default::default()),
                _ => w.rect,
            };
            let wb = Box2::new(
                card.x + (r.x as f64 - g.x) * k,
                card.y + (r.y as f64 - g.y) * k,
                r.w as f64 * k,
                r.h as f64 * k,
            );
            if wb.right() < card.x || wb.x > card.right() || wb.bottom() < card.y || wb.y > card.bottom() {
                continue;
            }
            let dim = !matches(l, id, &ov.filter);
            let a = if dragging == Some(id) {
                0.35
            } else if dim || parked.is_some() {
                // A parked Desk's windows draw nothing: shown faintly.
                0.25
            } else {
                1.0
            };
            // A small tab and frame.
            let tab = Box2::new(
                wb.x - 1.0,
                wb.y - (TAB_H as f64 * k).max(3.0),
                (wb.w * 0.4).clamp(6.0, 140.0),
                (TAB_H as f64 * k).max(3.0),
            );
            let focused = l.wm.focused() == Some(id) && desk == current;
            let tab_color = if focused { p.focus } else { p.tab_idle };
            pt.fill(tab, tab_color.with_alpha(a));
            pt.fill(Box2::new(wb.x - 1.0, wb.y - 1.0, wb.w + 2.0, wb.h + 2.0), p.bevel_lo.with_alpha(a));
            pt.fill(wb, p.surface.with_alpha(a));
            pt.thumbnail(az, id, wb, a);
            if focused {
                pt.fill(Box2::new(wb.x, wb.y, wb.w, 1.0), p.focus);
            }
            ov.thumbs.push((id, desk, Box2::new(tab.x, tab.y, wb.w.max(tab.w), wb.bottom() - tab.y)));
        }
        if desk == current {
            pt.outline(Box2::new(card.x - 3.0, card.y - 3.0, card.w + 6.0, card.h + 6.0), 2.0, p.focus);
        }
        // A thumbnail dragged over this card.
        if let Some((_, _, (px, py))) = ov.drag
            && card.contains(px, py)
            && dragging.and_then(|d| l.wm.get(d)).is_some_and(|w| w.desk != desk)
        {
            pt.dashed(Box2::new(card.x - 3.0, card.y - 3.0, card.w + 6.0, card.h + 6.0), 2.0, p.focus);
        }
    }
    // The dragged thumbnail follows the pointer, and over another Desk a
    // label says where it goes: "Notes → Mail".
    if let Some((id, from, at)) = ov.drag
        && let Some((_, _, b)) = ov.thumbs.iter().find(|(w, ..)| *w == id).cloned()
    {
        let moved = Box2::new(b.x + at.0 - from.0, b.y + at.1 - from.1, b.w, b.h);
        pt.fill(Box2::new(moved.x + 3.0, moved.y + 3.0, moved.w, moved.h), p.slab);
        pt.fill(moved, p.bevel_lo);
        pt.thumbnail(az, id, moved.inset(1.0), 1.0);
        let over = ov.cards.iter().find(|(_, c)| c.contains(at.0, at.1)).map(|(d, _)| *d);
        let from_desk = l.wm.get(id).map(|w| w.desk);
        let target = match over {
            Some(Some(d)) if Some(d) != from_desk => {
                let i = l.wm.desk_index(d).unwrap_or(0);
                Some(l.wm.desk(d).map_or_else(|| i.to_string(), |d| d.name.clone()))
            }
            Some(None) => Some("a new Desk".to_string()),
            _ => None,
        };
        if let Some(target) = target {
            let title = l.wm.get(id).map(|w| w.title.clone()).unwrap_or_default();
            let title: String =
                if title.chars().count() > 32 { title.chars().take(31).chain(['…']).collect() } else { title };
            let label = format!("{title} → {target}");
            let style = mono_bold(11.0, p.on_selection);
            let lw = az.labels.measure(&label, style.face, style.size()).0.ceil() + 16.0;
            let lb = Box2::new(moved.x, (moved.bottom() + 6.0).min(g.bottom() - 26.0), lw, 22.0);
            pt.fill(lb, p.selection);
            pt.text_mid(&mut az.labels, &label, mono_bold(11.0, p.on_selection), lb.x + 8.0, lb.y, lb.h, lw);
        }
    }

    // Hints.
    let hy = g.bottom() - 40.0;
    let mut hx = g.x + margin;
    for (k, what) in [
        ("❖ scroll", "zoom"),
        ("drag", "move a window to another Desk"),
        ("1…9", "go to a Desk"),
        ("p", "park a Desk"),
        ("type", "filter"),
        ("Esc", "close"),
    ] {
        hx += pt.text_mid(&mut az.labels, k, mono_bold(11.0, p.text2), hx, hy, 20.0, 80.0) + 6.0;
        hx += pt.text_mid(&mut az.labels, what, mono(11.0, p.text3), hx, hy, 20.0, 300.0) + 22.0;
    }

    let out = pt.finish(&mut ov.cache);
    l.overlays.overview = Some(ov);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_desk_says_what_it_is_doing() {
        assert_eq!(age(30), "just now");
        assert_eq!(age(12 * 60), "12 min");
        assert_eq!(age(3 * 3600 + 5), "3 h");
        assert_eq!(age(86_400), "1 day");
        assert_eq!(age(3 * 86_400), "3 days");
        let now = 10_000_000;
        assert_eq!(status(true, Some(now - 5), Some(now - 5), now).as_deref(), Some("here"));
        assert_eq!(
            status(false, Some(now - 3 * 86_400), Some(now), now).as_deref(),
            Some("parked 3 days · draws nothing")
        );
        assert_eq!(status(false, None, Some(now - 12 * 60), now).as_deref(), Some("12 min ago"));
        assert_eq!(status(false, None, Some(now - 10), now).as_deref(), Some("just now"));
        assert_eq!(status(false, None, None, now), None, "never left: nothing to say");
    }
}
