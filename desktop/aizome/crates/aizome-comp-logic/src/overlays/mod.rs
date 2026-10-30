//! What the compositor draws above windows: the stack-drop feedback, the
//! marking menu, the switcher, the Desks overview and the screenshot area
//! picker. Each draws in Aizome's look (square corners, 1pt bevels, hard
//! 3pt shadows, Archivo and IBM Plex Mono, the Night/Day palette).

pub mod area;
pub mod marking;
pub mod overview;
pub mod switcher;

use aizome_comp_core::logic::KeyDecision;
use aizome_comp_core::scene::{Scene, SceneItem};
use smithay::backend::renderer::element::Kind;
use smithay::input::keyboard::Keysym;
use smithay::output::Output;
use smithay::utils::{Logical, Physical, Point};

use crate::bindings::Mods;
use crate::decor::Piece;
use crate::prelude::*;
use aizome_comp_core::text::{Face, Style};

#[derive(Default)]
pub struct Overlays {
    pub marking: Option<marking::Marking>,
    pub switcher: Option<switcher::Switcher>,
    pub overview: Option<overview::Overview>,
    pub area: Option<area::AreaPick>,
    pub drag_cache: crate::decor::DecorCache,
    /// Counts marking menus opened (each one's pause timer knows its own).
    pub marking_opened: u32,
}

impl Overlays {
    /// The overview covers everything else on its output.
    pub fn covers_desktop(&self, output: &Output) -> bool {
        self.overview.as_ref().is_some_and(|o| &o.output == output)
    }

    /// While a modal overlay is up, the focused tab doesn't show focus.
    pub fn steals_focus_look(&self) -> bool {
        self.overview.is_some()
    }

    /// Keys go to the compositor, not the focused client.
    pub fn grabs_keyboard(&self) -> bool {
        self.switcher.is_some() || self.overview.is_some() || self.area.is_some()
    }

    pub fn animating(&self) -> bool {
        false
    }

    pub fn overview_open(&self) -> bool {
        self.overview.is_some()
    }

    /// Whether any is up (or, for the marking menu, waiting out its pause).
    pub fn any(&self) -> bool {
        self.marking.is_some() || self.switcher.is_some() || self.overview.is_some() || self.area.is_some()
    }
}

/// Overlays on an output, front first.
pub fn items(st: &mut State, output: &Output, scale: f64) -> Scene {
    let mut out = Vec::new();
    out.extend(area::items(st, output, scale));
    out.extend(marking::items(st, output, scale));
    out.extend(switcher::items(st, output, scale));
    out.extend(overview::items(st, output, scale));
    out
}

/// Closes every overlay (a reload is coming, or the recovery overlay
/// opened).
pub fn close_all(st: &mut State) {
    switcher::close(st, false);
    let l = st.lg.m();
    l.overlays.overview = None;
    l.overlays.marking = None;
    l.overlays.area = None;
    st.az.cursor_override = None;
    st.az.queue_redraw_all();
}

/// The session locked: every overlay closes, so none takes the keys or
/// clicks meant for the lock prompt (a ⌃❖Space menu left up would run its
/// verbs on the window behind it, and the switcher would wait for a ❖ the
/// prompt has). A picker someone asked for hears nothing was picked, as
/// with Esc. Called between loop iterations, so right after the lock
/// engages; the input paths below check the lock too.
pub fn close_for_lock(st: &mut State) {
    if !st.az.lock.is_locked() || !st.lg.r().overlays.any() {
        return;
    }
    tracing::debug!("the session locked: overlays close");
    area::cancel(st);
    switcher::close(st, false);
    overview::close(st);
    marking::close(st);
    st.update_keyboard_focus();
}

/// A key while an overlay is up; `None` lets it through. Behind the lock
/// every key does: they're the prompt's.
pub fn key(state: &mut State, syms: &[Keysym], mods: Mods, pressed: bool, utf8: Option<char>) -> Option<KeyDecision> {
    if state.az.lock.is_locked() {
        return None;
    }
    if state.lg.m().overlays.area.is_some() {
        return area::key(state, syms, pressed);
    }
    if state.lg.m().overlays.switcher.is_some() {
        return switcher::key(state, syms, mods, pressed);
    }
    if state.lg.m().overlays.overview.is_some() {
        return overview::key(state, syms, mods, pressed, utf8);
    }
    if state.lg.m().overlays.marking.as_ref().is_some_and(|m| m.shown) {
        return marking::key(state, syms, pressed);
    }
    None
}

/// Scrolling over an overlay; true if it took it. Behind the lock the
/// pointer, like the keys, is the prompt's: no overlay scrolls, follows
/// it or takes a button.
pub fn scroll(state: &mut State, _h: f64, v: f64) -> bool {
    if state.az.lock.is_locked() {
        return false;
    }
    overview::scroll(state, v) || state.lg.m().overlays.switcher.is_some()
}

pub fn pointer_moved(state: &mut State, pos: Point<f64, Logical>) {
    if state.az.lock.is_locked() {
        return;
    }
    area::pointer_moved(state, pos.x, pos.y);
    let grabbed = state.az.seat.get_pointer().is_some_and(|p| p.is_grabbed());
    if !grabbed && state.lg.m().overlays.marking.as_ref().is_some_and(|m| m.sticky && m.shown) {
        marking::moved(state, pos.x, pos.y);
    }
    overview::pointer_moved(state, pos.x, pos.y);
}

/// A button over an overlay; true if it took it.
pub fn pointer_button(state: &mut State, button: u32, pressed: bool, pos: Point<f64, Logical>) -> bool {
    let _ = button;
    if state.az.lock.is_locked() {
        return false;
    }
    if state.lg.m().overlays.area.is_some() {
        return area::pointer_button(state, pressed, pos.x, pos.y);
    }
    if state.lg.m().overlays.overview.is_some() {
        return overview::pointer_button(state, pressed, pos.x, pos.y);
    }
    if state.lg.m().overlays.switcher.is_some() {
        if pressed {
            return switcher::click(state, pos.x, pos.y);
        }
        return true;
    }
    if state.lg.m().overlays.marking.as_ref().is_some_and(|m| m.sticky && m.shown) {
        if pressed {
            marking::click(state, pos.x, pos.y);
        }
        return true;
    }
    false
}

pub fn toggle_overview(state: &mut State) {
    overview::toggle(state);
}

pub fn open_switcher(state: &mut State, reverse: bool, held: bool) {
    if state.lg.m().overlays.overview.is_some() {
        return;
    }
    switcher::open(state, reverse, held);
}

pub fn close_switcher(state: &mut State, activate: bool) {
    switcher::close(state, activate);
}

/// The area picker: for a screenshot, or (`report`) to tell the shell or the
/// share picker what was picked.
pub fn start_area_pick(state: &mut State, report: Option<area::Report>) {
    area::start(state, report);
}

/// A window closed: overlays forget it.
pub fn window_gone(l: &mut Logic, id: WinId) {
    if let Some(sw) = &mut l.overlays.switcher {
        for app in &mut sw.apps {
            app.windows.retain(|w| *w != id);
        }
        sw.apps.retain(|a| !a.windows.is_empty());
        if sw.apps.is_empty() {
            l.overlays.switcher = None;
        } else {
            sw.app = sw.app.min(sw.apps.len() - 1);
            sw.win = sw.win.min(sw.apps[sw.app].windows.len().saturating_sub(1));
        }
    }
    if l.overlays.marking.as_ref().is_some_and(|m| m.win == id) {
        l.overlays.marking = None;
    }
    if let Some(ov) = &mut l.overlays.overview
        && ov.drag.is_some_and(|d| d.0 == id)
    {
        ov.drag = None;
    }
}

/// Feedback for a tab being dragged: drawn under the top layer.
pub fn drag_feedback(st: &mut State, output: &Output, scale: f64) -> Scene {
    let mut out = Vec::new();
    let (az, l) = (&mut st.az, st.lg.m());
    let Some(drag) = l.interactive.drag.clone() else { return out };
    let (Some(slot), Some(target)) = (drag.slot, drag.target) else { return out };
    let geo = az.output_geometry(output);
    let origin = (geo.loc.x as f64, geo.loc.y as f64);
    if !az.output_rect(output).intersects(slot.grow(60)) {
        return out;
    }
    let p = az.palette;
    // The label above the slot: "Release to stack with ‹title›".
    let title = l.wm.get(target).map(|w| w.title.clone()).unwrap_or_default();
    let text = format!("Release to stack with {title}");
    let text = az.labels.fit(&text, Face::UiSemibold, 12.0, 420.0);
    let style = Style::new(Face::UiSemibold, 12.0, p.on_menu_title);
    let mut pieces = Vec::new();
    // Slot: focus at 18% with a 2pt dashed focus border.
    pieces.push(Piece::rect(slot, p.focus.with_alpha(0.18)));
    dashed(&mut pieces, slot, 2.0, 6.0, 4.0, p.focus);
    if let Some(label) = az.labels.get(&text, style, scale, 440.0) {
        let (w, h) = (label.w as f64 + 16.0, 22.0);
        let x = slot.x as f64;
        let y = slot.y as f64 - h - 18.0;
        pieces.push(Piece::new(x, y, w, h, p.menu_title));
        let loc: Point<i32, Physical> =
            Point::<f64, Logical>::from((x + 8.0 - origin.0, y + ((h - label.h as f64) / 2.0).round() - origin.1))
                .to_physical_precise_round(scale);
        out.push(SceneItem::Memory {
            buffer: label.buffer.clone(),
            loc: loc.to_f64(),
            alpha: None,
            src: None,
            size: Some((label.w, label.h).into()),
            kind: Kind::Unspecified,
        });
    }
    out.extend(l.overlays.drag_cache.elements(&pieces, origin, scale, 1.0).into_iter().map(SceneItem::Solid));
    out
}

pub use aizome_comp_core::pieces::dashed;
