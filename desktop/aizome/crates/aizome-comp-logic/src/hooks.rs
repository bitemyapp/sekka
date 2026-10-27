//! The table the core calls through, and the hooks behind it. Every hook
//! that may change the model ends by publishing the view the core keeps
//! ([`publish`]).

use std::any::TypeId;

use aizome_comp_core::aizome_config::Settings;
use aizome_comp_core::aizome_ipc::{Request, Response};
use aizome_comp_core::logic::{AxisEvent, KeyDecision, KeyEvent, LogicVTable, MapInfo, ToplevelRequest};
use aizome_comp_core::scene::{Purpose, Scene};
use aizome_comp_core::view::{View, WinView};
use smithay::output::Output;
use smithay::utils::{Logical, Point};
use smithay::wayland::shell::xdg::ToplevelSurface;

use crate::prelude::*;
use crate::{Saved, install, uninstall};

pub static VTABLE: LogicVTable = LogicVTable {
    abi: aizome_comp_core::ABI,
    build: concat!(env!("CARGO_PKG_VERSION"), "+", env!("AIZOME_LOGIC_BUILD")),
    state_type,
    init,
    save,
    quiesce,
    release,
    publish,
    first_configure,
    window_mapped,
    window_committed,
    window_unmapped,
    toplevel_request,
    usable_area_changed,
    outputs_changed,
    key,
    decide_key,
    key_action,
    pointer_motion,
    pointer_button,
    pointer_axis,
    touch_down,
    scene,
    window_bounds,
    request,
    timer,
    settings_changed,
    loop_iteration,
};

fn state_type() -> TypeId {
    TypeId::of::<State>()
}

// ---- life ----------------------------------------------------------------------

fn init(st: &mut State, saved: Option<&[u8]>) -> Result<(), String> {
    let mut logic = Logic::new(&st.az.settings);
    let mut slides = Vec::new();
    if let Some(bytes) = saved {
        let saved: Saved = serde_json::from_slice(bytes).map_err(|e| format!("the saved state: {e}"))?;
        slides = saved.slides;
        logic.audible = saved.audible.into_iter().collect();
        if let Some(model) = saved.model {
            logic.wm = model;
        }
        for (id, size) in saved.requested_sizes {
            logic.data(id).requested_size = Some(size);
        }
    }
    install(&mut st.lg, logic);
    crate::actions::adopt_windows(st);
    crate::actions::sync_screens(st);
    crate::slides::restore(st, &slides);
    publish(st);
    st.update_keyboard_focus();
    tracing::info!(build = VTABLE.build, windows = st.lg.r().wm.windows().count(), "compositor logic started");
    Ok(())
}

fn save(st: &mut State) -> Result<Vec<u8>, String> {
    let l = st.lg.r();
    let saved = Saved {
        model: Some(l.wm.clone()),
        requested_sizes: l.wins.iter().filter_map(|(id, d)| d.requested_size.map(|s| (*id, s))).collect(),
        slides: crate::slides::save(l),
        audible: l.audible.iter().copied().collect(),
    };
    serde_json::to_vec(&saved).map_err(|e| e.to_string())
}

fn quiesce(st: &mut State) {
    crate::overlays::close_all(st);
    let l = st.lg.m();
    l.interactive = Default::default();
    l.pending_action = None;
    l.anims = Default::default();
    let access = std::mem::take(&mut l.access);
    for t in access.slow.into_values().flatten() {
        st.cancel_timer(t);
    }
    st.az.cursor_override = None;
}

fn release(st: &mut State) {
    drop(uninstall(&mut st.lg));
}

/// Tells the core where windows are, what shows and what has focus, when
/// any of it changed.
pub fn publish(st: &mut State) {
    // Windows the model fitted to a display hear about it first.
    crate::actions::apply_relaid(st);
    let l = st.lg.m();
    let grabs = l.overlays.grabs_keyboard();
    let animating = l.anims.active() || l.overlays.animating() || l.slides.active();
    // Cards going leave their last frame to slide out, when there's motion.
    let keep = st.az.motion_ms() > 0;
    let l = st.lg.m();
    // Called on every commit and frame: only the model's generation and three
    // flags are looked at until something changed (the Desk's folder is
    // the model's too).
    let now = (l.wm.generation(), grabs, animating, keep);
    if l.published == Some(now) {
        return;
    }
    l.published = Some(now);
    let folder =
        l.wm.desk(l.wm.current_desk())
            .and_then(|d| d.folder.clone())
            .map(|f| aizome_comp_core::aizome_config::expand_home(&f));
    let view = View {
        focused: l.wm.focused(),
        stacking: l.wm.shown_stacking(),
        mru: l.wm.mru().to_vec(),
        grabs_keyboard: grabs,
        animating,
        outset: crate::decor::OUTSET,
        desk_folder: folder,
        keep_on_unmap: if keep { vec![crate::slides::NOTES.to_owned()] } else { Vec::new() },
    };
    let wins: Vec<(WinId, WinView)> =
        l.wm.windows()
            .map(|w| {
                (
                    w.id,
                    WinView {
                        rect: w.rect,
                        shown: l.wm.is_shown(w.id),
                        minimised: w.minimised,
                        hidden: w.hidden,
                        fill: w.fill(),
                        fullscreen: w.fullscreen(),
                        urgent: w.urgent,
                        parent: w.parent,
                        desk: if w.sticky { None } else { l.wm.desk_index(w.desk) },
                    },
                )
            })
            .collect();
    st.publish_view(view, wins);
}

// ---- windows -------------------------------------------------------------------

fn first_configure(st: &mut State, toplevel: &ToplevelSurface) {
    crate::actions::first_configure(st, toplevel);
}

fn window_mapped(st: &mut State, id: WinId, info: &MapInfo) {
    crate::actions::window_mapped(st, id, info);
    publish(st);
}

fn window_committed(st: &mut State, id: WinId, is_root: bool) {
    crate::actions::window_committed(st, id, is_root);
    publish(st);
}

fn window_unmapped(st: &mut State, id: WinId) {
    crate::actions::window_unmapped(st, id);
    publish(st);
}

fn toplevel_request(st: &mut State, id: WinId, req: ToplevelRequest) {
    crate::actions::toplevel_request(st, id, req);
    publish(st);
}

fn usable_area_changed(st: &mut State, output: &Output) {
    crate::actions::sync_screens(st);
    crate::actions::refit_placed_windows(st, output);
    publish(st);
}

fn outputs_changed(st: &mut State) {
    crate::actions::sync_screens(st);
    crate::actions::rescue_windows(st);
    if !st.shell_restart_holds_layout() {
        for output in st.az.outputs.clone() {
            crate::actions::refit_placed_windows(st, &output);
        }
    }
    publish(st);
}

// ---- input ---------------------------------------------------------------------

fn key(st: &mut State, code: u32, pressed: bool, time: u32) {
    st.logic_key(code, pressed, time);
    publish(st);
}

fn decide_key(st: &mut State, ev: &KeyEvent<'_>) -> KeyDecision {
    st.decide_key(ev)
}

fn key_action(st: &mut State) {
    if let Some(a) = st.lg.m().pending_action.take() {
        st.run_action(a);
    }
    publish(st);
}

fn pointer_motion(st: &mut State, pos: Point<f64, Logical>, time: u32) {
    st.logic_pointer_motion(pos, time);
    publish(st);
}

fn pointer_button(st: &mut State, button: u32, pressed: bool, time: u32) {
    st.logic_pointer_button(button, pressed, time);
    publish(st);
}

fn pointer_axis(st: &mut State, a: &AxisEvent) {
    st.logic_pointer_axis(a);
    publish(st);
}

fn touch_down(st: &mut State, pos: Point<f64, Logical>) {
    st.logic_touch_down(pos);
    publish(st);
}

// ---- frames --------------------------------------------------------------------

fn scene(st: &mut State, output: &Output, purpose: Purpose) -> Scene {
    st.lg.m().anims.tick();
    publish(st);
    crate::render::scene(st, output, purpose)
}

fn window_bounds(st: &mut State, id: WinId) -> Option<Rect> {
    let shadows = st.az.settings.appearance.effects.shadows();
    let l = st.lg.r();
    match l.decor_layout(&mut st.az.labels, id) {
        Some(layout) => {
            let mut r = layout.frame;
            for t in &layout.tabs {
                r = r.union(t.rect);
            }
            let shadow = if shadows { crate::decor::SHADOW } else { 0 };
            Some(Rect::new(r.x, r.y, r.w + shadow, r.h + shadow))
        }
        None => l.wm.get(id).map(|w| w.rect),
    }
}

// ---- the rest ------------------------------------------------------------------

fn request(st: &mut State, req: &Request) -> Option<Response> {
    let r = st.wm_request(req);
    publish(st);
    r
}

/// Timer tokens: the kind in the high half.
pub const TIMER_MARKING: u64 = 1 << 32;
pub const TIMER_SLOW_KEY: u64 = 2 << 32;

fn timer(st: &mut State, token: u64) {
    match token & !0xffff_ffff {
        TIMER_MARKING => crate::overlays::marking::pause_over(st, token & 0xffff_ffff),
        TIMER_SLOW_KEY => st.slow_key_counted((token & 0xffff_ffff) as u32),
        _ => {}
    }
    publish(st);
}

fn settings_changed(st: &mut State, old: &Settings) {
    crate::config::settings_changed(st, old);
    publish(st);
}

fn loop_iteration(st: &mut State) {
    crate::actions::flush_ipc_events(st);
    crate::scales::refresh(st);
    st.az.labels.gc();
}
