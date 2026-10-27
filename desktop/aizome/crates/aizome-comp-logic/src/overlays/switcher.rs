//! The switcher (❖Tab, ❖⇧Tab while ❖ is held), C1's two tiers in
//! Aizome's look: apps with windows across the top, most recently used
//! first, each with its icon, name and window count; below, the chosen
//! app's windows as live thumbnails with their titles and where they are.
//!
//! Tab moves through the apps (⇧ back); ` or ↓ goes down to the windows
//! and moves through them; ↑ goes back up; releasing ❖ (or Return)
//! activates; Esc cancels; Q quits the app (closes all its windows), W
//! closes the window, H hides the app.

use smithay::input::keyboard::{Keysym, keysyms};
use smithay::output::Output;

use crate::bindings::Mods;
use crate::decor::DecorCache;
use crate::prelude::*;
use crate::wm::WinId;
use aizome_comp_core::icons::Tint;
use aizome_comp_core::logic::KeyDecision;
use aizome_comp_core::painter::{Box2, Painter, bold, mono, semibold};

pub struct App {
    pub app_id: String,
    pub name: String,
    pub windows: Vec<WinId>,
}

pub struct Switcher {
    pub apps: Vec<App>,
    pub app: usize,
    /// In the windows row.
    pub windows_tier: bool,
    pub win: usize,
    /// Opened by a chord with ❖: releasing ❖ picks.
    pub held: bool,
    pub output: Output,
    pub cache: DecorCache,
    /// Hit boxes from the last drawing.
    pub app_boxes: Vec<Box2>,
    pub win_boxes: Vec<Box2>,
}

const TILE_W: f64 = 92.0;
const TILE_H: f64 = 100.0;
const ICON: i32 = 48;
const CARD_W: f64 = 200.0;
const THUMB_H: f64 = 124.0;
const CARD_H: f64 = THUMB_H + 44.0;
const PAD: f64 = 16.0;
const GAP: f64 = 8.0;
const MAX_CARDS: usize = 4;

pub fn open(state: &mut State, reverse: bool, held: bool) {
    if let Some(sw) = &mut state.lg.m().overlays.switcher {
        step_app(sw, if reverse { -1 } else { 1 });
        let o = sw.output.clone();
        state.az.queue_redraw(&o);
        return;
    }
    // On the display with the focus, where the keys are.
    let Some(output) = crate::actions::desk_output(state, state.lg.r().wm.current_desk()) else { return };
    let l = state.lg.m();
    let apps: Vec<App> =
        l.wm.apps_mru()
            .into_iter()
            .map(|(app_id, windows)| {
                let name = state.az.icons.app_name(&app_id).unwrap_or_else(|| short_name(&app_id));
                App { app_id, name, windows }
            })
            .collect();
    if apps.is_empty() {
        return;
    }
    // The next app (the first is the current one), or the last going back.
    let n = apps.len();
    let app = if n == 1 {
        0
    } else if reverse {
        n - 1
    } else {
        1
    };
    state.lg.m().overlays.switcher = Some(Switcher {
        apps,
        app,
        windows_tier: false,
        win: 0,
        held,
        output: output.clone(),
        cache: DecorCache::default(),
        app_boxes: Vec::new(),
        win_boxes: Vec::new(),
    });
    let ms = state.az.motion_ms();
    state.lg.m().anims.start_overlay("switcher", ms);
    state.update_keyboard_focus();
    state.az.queue_redraw(&output);
}

fn short_name(app_id: &str) -> String {
    let last = app_id.rsplit('.').next().unwrap_or(app_id);
    let mut c = last.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => app_id.to_string(),
    }
}

fn step_app(sw: &mut Switcher, d: isize) {
    let n = sw.apps.len() as isize;
    sw.app = ((sw.app as isize + d).rem_euclid(n)) as usize;
    sw.windows_tier = false;
    sw.win = 0;
}

fn step_win(sw: &mut Switcher, d: isize) {
    let n = sw.apps[sw.app].windows.len() as isize;
    if n == 0 {
        return;
    }
    if !sw.windows_tier {
        sw.windows_tier = true;
        // The first move down picks the app's second window (the first is
        // where you are).
        sw.win = if n > 1 && d > 0 { 1 } else { 0 };
        return;
    }
    sw.win = ((sw.win as isize + d).rem_euclid(n)) as usize;
}

pub fn close(state: &mut State, activate: bool) {
    let Some(sw) = state.lg.m().overlays.switcher.take() else { return };
    state.az.queue_redraw(&sw.output);
    if activate {
        let app = &sw.apps[sw.app];
        let target = if sw.windows_tier { app.windows.get(sw.win).copied() } else { app.windows.first().copied() };
        if let Some(w) = target {
            if state.lg.m().wm.get(w).is_some_and(|w| w.hidden) {
                let a = app.app_id.clone();
                state.show_app(&a);
            }
            state.activate_window(w);
        }
    }
    state.update_keyboard_focus();
}

/// Keys while the switcher is up.
pub fn key(state: &mut State, syms: &[Keysym], mods: Mods, pressed: bool) -> Option<KeyDecision> {
    let sw = state.lg.m().overlays.switcher.as_mut()?;
    let is = |k: u32| syms.iter().any(|s| s.raw() == k);
    if !pressed {
        if sw.held && (is(keysyms::KEY_Super_L) || is(keysyms::KEY_Super_R)) {
            close(state, true);
        }
        return Some(KeyDecision::Swallow);
    }
    if is(keysyms::KEY_Tab) || is(keysyms::KEY_ISO_Left_Tab) {
        step_app(sw, if mods.shift || is(keysyms::KEY_ISO_Left_Tab) { -1 } else { 1 });
    } else if is(keysyms::KEY_grave) || is(keysyms::KEY_Down) {
        step_win(sw, if mods.shift { -1 } else { 1 });
    } else if is(keysyms::KEY_Up) {
        sw.windows_tier = false;
    } else if is(keysyms::KEY_Right) {
        if sw.windows_tier { step_win(sw, 1) } else { step_app(sw, 1) }
    } else if is(keysyms::KEY_Left) {
        if sw.windows_tier { step_win(sw, -1) } else { step_app(sw, -1) }
    } else if is(keysyms::KEY_Return) || is(keysyms::KEY_KP_Enter) {
        close(state, true);
        return Some(KeyDecision::Swallow);
    } else if is(keysyms::KEY_Escape) {
        close(state, false);
        return Some(KeyDecision::Swallow);
    } else if is(keysyms::KEY_q) {
        // Quit the app: ask every window of it to close.
        let ws = sw.apps[sw.app].windows.clone();
        for w in ws {
            state.close_window(w);
        }
        close(state, false);
        return Some(KeyDecision::Swallow);
    } else if is(keysyms::KEY_w) {
        let app = &sw.apps[sw.app];
        let w = if sw.windows_tier { app.windows.get(sw.win).copied() } else { app.windows.first().copied() };
        if let Some(w) = w {
            state.close_window(w);
        }
        close(state, false);
        return Some(KeyDecision::Swallow);
    } else if is(keysyms::KEY_h) {
        let a = sw.apps[sw.app].app_id.clone();
        close(state, false);
        state.hide_app(&a);
        return Some(KeyDecision::Swallow);
    } else if is(keysyms::KEY_Super_L)
        || is(keysyms::KEY_Super_R)
        || is(keysyms::KEY_Shift_L)
        || is(keysyms::KEY_Shift_R)
    {
        // Modifiers alone change nothing.
    }
    if let Some(sw) = &state.lg.m().overlays.switcher {
        let o = sw.output.clone();
        state.az.queue_redraw(&o);
    }
    Some(KeyDecision::Swallow)
}

/// A click on a tile or card picks it.
pub fn click(state: &mut State, x: f64, y: f64) -> bool {
    let Some(sw) = state.lg.m().overlays.switcher.as_mut() else { return false };
    if let Some(i) = sw.app_boxes.iter().position(|b| b.contains(x, y)) {
        sw.app = i;
        sw.windows_tier = false;
        close(state, true);
        return true;
    }
    if let Some(i) = sw.win_boxes.iter().position(|b| b.contains(x, y)) {
        sw.windows_tier = true;
        sw.win = i;
        close(state, true);
        return true;
    }
    // Clicks elsewhere close it.
    close(state, false);
    true
}

pub fn items(st: &mut State, output: &Output, scale: f64) -> aizome_comp_core::scene::Scene {
    let (az, l) = (&mut st.az, st.lg.m());
    let Some(mut sw) = l.overlays.switcher.take() else { return Vec::new() };
    if &sw.output != output {
        l.overlays.switcher = Some(sw);
        return Vec::new();
    }
    let geo = az.output_geometry(output);
    let origin = (geo.loc.x as f64, geo.loc.y as f64);
    let alpha = l.anims.overlay_progress("switcher") as f32;
    let mut pt: Painter = Painter::new(origin, scale, alpha);
    let p = az.palette;
    let shadows = az.settings.appearance.effects.shadows();

    let n_apps = sw.apps.len();
    let wins = sw.apps[sw.app].windows.clone();
    let n_cards = wins.len().clamp(1, MAX_CARDS);
    let apps_w = n_apps as f64 * (TILE_W + GAP) - GAP;
    let cards_w = n_cards as f64 * (CARD_W + 12.0) - 12.0;
    let max_w = geo.size.w as f64 - 64.0;
    let w = (apps_w.max(cards_w).max(520.0) + 2.0 * PAD).min(max_w);
    let h = PAD + TILE_H + 14.0 + 30.0 + CARD_H + 14.0 + 30.0 + PAD - 8.0;
    let x0 = origin.0 + ((geo.size.w as f64 - w) / 2.0).round();
    let y0 = origin.1 + ((geo.size.h as f64 - h) / 2.0).round();
    pt.panel(Box2::new(x0, y0, w, h), p.raised, &p, shadows);

    // Apps.
    sw.app_boxes.clear();
    let first = if apps_w > w - 2.0 * PAD {
        // Keep the chosen app in view.
        let fit = ((w - 2.0 * PAD + GAP) / (TILE_W + GAP)).floor().max(1.0) as usize;
        sw.app.saturating_sub(fit - 1).min(n_apps.saturating_sub(fit))
    } else {
        0
    };
    let row_w = (n_apps - first) as f64 * (TILE_W + GAP) - GAP;
    let mut x = x0 + ((w - row_w.min(w - 2.0 * PAD)) / 2.0).round();
    let y = y0 + PAD;
    for i in 0..first {
        let _ = i;
        sw.app_boxes.push(Box2::default());
    }
    for (i, app) in sw.apps.iter().enumerate().skip(first) {
        if x + TILE_W > x0 + w - PAD + 0.5 {
            sw.app_boxes.push(Box2::default());
            continue;
        }
        let tile = Box2::new(x, y, TILE_W, TILE_H);
        sw.app_boxes.push(tile);
        let chosen = i == sw.app;
        let (ink, sub) = if chosen && !sw.windows_tier {
            pt.fill(tile, p.selection);
            pt.outline(tile, 2.0, p.focus);
            (p.on_selection, p.on_selection)
        } else if chosen {
            pt.outline(tile, 1.0, p.focus);
            (p.text, p.text3)
        } else {
            (p.text, p.text3)
        };
        let all_hidden = app.windows.iter().all(|w| l.wm.get(*w).is_some_and(|w| w.hidden || w.minimised));
        let icon_alpha = if all_hidden { 0.45 } else { 1.0 };
        pt.icon(&mut az.icons, &app.app_id, Tint::Color, x + (TILE_W - ICON as f64) / 2.0, y + 8.0, ICON, icon_alpha);
        pt.text_center(
            &mut az.labels,
            &app.name,
            semibold(12.0, ink),
            Box2::new(x + 4.0, y + 62.0, TILE_W - 8.0, 16.0),
        );
        let n = app.windows.len();
        let count = if all_hidden {
            "hidden".to_string()
        } else if n == 1 {
            "1 window".into()
        } else {
            format!("{n} windows")
        };
        pt.text_center(&mut az.labels, &count, mono(10.5, sub), Box2::new(x + 2.0, y + 80.0, TILE_W - 4.0, 14.0));
        x += TILE_W + GAP;
    }

    // The divider.
    let dy = y0 + PAD + TILE_H + 7.0;
    pt.fill(Box2::new(x0 + PAD, dy, w - 2.0 * PAD, 1.0), p.line);
    pt.fill(Box2::new(x0 + PAD, dy + 1.0, w - 2.0 * PAD, 1.0), p.bevel_hi);

    // The chosen app's windows.
    let app = &sw.apps[sw.app];
    let hy = dy + 8.0;
    let nw = pt.text_mid(&mut az.labels, &app.name, bold(13.0, p.text), x0 + PAD, hy, 28.0, 240.0);
    let n = app.windows.len();
    let hint = format!(
        "{} · ↓ or keep holding to pick one",
        if n == 1 { "1 window".to_string() } else { format!("{n} windows") }
    );
    pt.text_mid(&mut az.labels, &hint, mono(11.0, p.text3), x0 + PAD + nw + 10.0, hy, 28.0, w - 2.0 * PAD - nw - 10.0);
    sw.win_boxes.clear();
    let cy = hy + 30.0;
    let first_card = if sw.windows_tier { sw.win.saturating_sub(MAX_CARDS - 1) } else { 0 };
    let mut cx = x0 + PAD;
    for (i, wid) in wins.iter().enumerate() {
        if i < first_card || i >= first_card + MAX_CARDS {
            sw.win_boxes.push(Box2::default());
            continue;
        }
        let card = Box2::new(cx, cy, CARD_W, CARD_H);
        sw.win_boxes.push(card);
        let chosen = sw.windows_tier && i == sw.win;
        let thumb = Box2::new(cx, cy, CARD_W, THUMB_H);
        pt.fill(thumb, p.bevel_lo);
        pt.fill(thumb.inset(1.0), p.sunk);
        let w_info = l.wm.get(*wid).cloned();
        let faded = w_info.as_ref().is_some_and(|w| w.minimised || w.hidden);
        pt.thumbnail(az, *wid, thumb.inset(6.0), if faded { 0.5 } else { 1.0 });
        if chosen {
            pt.outline(Box2::new(cx - 3.0, cy - 3.0, CARD_W + 6.0, CARD_H + 6.0), 2.0, p.focus);
        }
        let title = w_info.as_ref().map(|w| w.title.clone()).unwrap_or_default();
        pt.text(&mut az.labels, &title, semibold(12.0, p.text), cx + 2.0, cy + THUMB_H + 6.0, CARD_W - 4.0);
        let place = match &w_info {
            Some(w) if w.minimised => "Minimised".to_string(),
            Some(w) if w.hidden => "Hidden".to_string(),
            Some(w) if w.sticky => "Every Desk".to_string(),
            Some(w) if w.desk == l.wm.current_desk() => "This Desk".to_string(),
            Some(w) => {
                let i = l.wm.desk_index(w.desk).unwrap_or(0);
                let name = l.wm.desk(w.desk).map(|d| d.name.clone()).unwrap_or_default();
                if name == i.to_string() { format!("Desk {i}") } else { format!("Desk {i} · {name}") }
            }
            None => String::new(),
        };
        pt.text(&mut az.labels, &place, mono(11.0, p.text3), cx + 2.0, cy + THUMB_H + 25.0, CARD_W - 4.0);
        cx += CARD_W + 12.0;
    }

    // Key hints.
    let fy = cy + CARD_H + 10.0;
    pt.fill(Box2::new(x0 + PAD, fy, w - 2.0 * PAD, 1.0), p.line);
    let mut hx = x0 + PAD;
    let hy = fy + 8.0;
    for (cap, what) in [
        ("❖Tab", "next app"),
        ("`", "windows"),
        ("W", "close window"),
        ("H", "hide app"),
        ("Q", "quit app"),
        ("Esc", "cancel"),
    ] {
        let (cw, _) = az.labels.measure(cap, aizome_comp_core::text::Face::Mono, 11.0);
        let capbox = Box2::new(hx, hy, cw.ceil() + 10.0, 20.0);
        if capbox.right() > x0 + w - PAD {
            break;
        }
        pt.outline(capbox, 1.0, p.line2);
        pt.text_center(&mut az.labels, cap, mono(11.0, p.text2), capbox);
        let tw = pt.text_mid(&mut az.labels, what, mono(11.0, p.text3), capbox.right() + 6.0, hy, 20.0, 160.0);
        hx = capbox.right() + 6.0 + tw + 16.0;
    }

    let out = pt.finish(&mut sw.cache);
    l.overlays.switcher = Some(sw);
    out
}
