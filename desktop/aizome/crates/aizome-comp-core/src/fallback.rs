//! The recovery path: what the core does on its own, so the session goes
//! on whatever the logic does.
//!
//! - **Without a logic** (none loaded, or one set aside after panicking
//!   again and again), windows show where the logic last put them (new
//!   ones centred), without decorations; a click focuses and raises one;
//!   keys go to the focused window; IPC lists windows, focuses and closes
//!   them. A reload brings a logic back, which takes the windows in.
//! - **The recovery overlay** (⌃⌥Delete, `recovery/`) is the core's too, so
//!   it works whatever the logic does.

use aizome_ipc::{Request, Response, WindowInfo};
use smithay::desktop::{WindowSurfaceType, layer_map_for_output};
use smithay::input::pointer::{AxisFrame, MotionEvent};
use smithay::output::Output;
use smithay::utils::{Logical, Point, SERIAL_COUNTER};
use smithay::wayland::shell::wlr_layer::{KeyboardInteractivity, Layer};

use crate::focus::Focus;
use crate::input::{Owner, SurfaceAt};
use crate::logic::{AxisEvent, MapInfo, ToplevelRequest};
use crate::scene::Scene;
use crate::state::State;
use crate::{Rect, WinId};

// ---- windows without a logic -------------------------------------------------

/// A window maps with no logic running: centred on the active output,
/// focused.
pub fn window_mapped(st: &mut State, id: WinId, info: &MapInfo) {
    let usable = st.az.active_output().map(|o| st.az.usable_area(&o)).unwrap_or(Rect::new(0, 0, 1280, 800));
    let (w, h) = info.size;
    let rect = Rect::new(usable.x + (usable.w - w).max(0) / 2, usable.y + (usable.h - h).max(0) / 2, w, h);
    if let Some(m) = st.az.windows.get_mut(&id) {
        m.view.rect = rect;
        m.view.shown = true;
    }
    st.az.view.stacking.push(id);
    focus(st, Some(id));
}

pub fn window_committed(st: &mut State, id: WinId) {
    let Some(m) = st.az.windows.get_mut(&id) else { return };
    let geo = m.window.geometry();
    let old = m.view.rect;
    m.view.rect.w = geo.size.w.max(1);
    m.view.rect.h = geo.size.h.max(1);
    st.az.queue_redraw_rect(old);
    st.redraw_window_now(id);
}

pub fn window_unmapped(st: &mut State, id: WinId) {
    st.az.view.stacking.retain(|w| *w != id);
    if st.az.view.focused == Some(id) {
        let next = st.az.view.stacking.last().copied();
        focus(st, next);
    }
}

pub fn toplevel_request(st: &mut State, id: WinId, req: ToplevelRequest) {
    match req {
        ToplevelRequest::Activate { .. } => focus(st, Some(id)),
        ToplevelRequest::Maximize(_) | ToplevelRequest::Fullscreen(_) => {
            // Acknowledged, not granted.
            if let Some(m) = st.az.windows.get(&id) {
                m.toplevel().send_configure();
            }
        }
        _ => {}
    }
}

/// Focuses a window (or nothing) and raises it.
fn focus(st: &mut State, id: Option<WinId>) {
    let prev = st.az.view.focused;
    if let Some(id) = id {
        st.az.view.stacking.retain(|w| *w != id);
        st.az.view.stacking.push(id);
    }
    st.az.view.focused = id;
    for (w, on) in [(prev, false), (id, true)] {
        if let Some(m) = w.and_then(|w| st.az.windows.get(&w)) {
            m.window.set_activated(on);
            m.toplevel().send_pending_configure();
        }
    }
    if id.is_some() {
        st.az.layer_focus = None;
    }
    st.az.window_outputs_dirty = true;
    st.update_keyboard_focus();
    st.az.queue_redraw_all();
}

/// What IPC can still do without a logic.
pub fn request(st: &mut State, req: &Request) -> Result<Response, String> {
    match req {
        Request::Windows => {
            let windows = st.az.view.stacking.iter().filter_map(|id| window_info(st, *id)).collect();
            Ok(Response::Windows { windows })
        }
        Request::FocusedWindow => {
            Ok(Response::Window { window: st.az.view.focused.and_then(|id| window_info(st, id)) })
        }
        Request::Focus { id } => {
            if !st.az.windows.contains_key(id) {
                return Err(format!("no window {id}"));
            }
            focus(st, Some(*id));
            Ok(Response::Ok)
        }
        Request::Close { id } => {
            let id = id.or(st.az.view.focused).ok_or("no window has focus")?;
            let m = st.az.windows.get(&id).ok_or_else(|| format!("no window {id}"))?;
            m.toplevel().send_close();
            Ok(Response::Ok)
        }
        _ => Err("the compositor's logic isn't running (it failed to load or kept failing); \
                  `aizomectl reload` loads it again, `aizomectl stats` says why"
            .into()),
    }
}

fn window_info(st: &State, id: WinId) -> Option<WindowInfo> {
    let m = st.az.windows.get(&id)?;
    let (app_id, title) = crate::shell::toplevel_ids(m.toplevel());
    let pid = st.az.dh.get_client(m.surface().id()).ok().and_then(|c| c.get_credentials(&st.az.dh).ok()).map(|c| c.pid);
    let r = m.view.rect;
    Some(WindowInfo {
        id,
        app_id,
        title,
        pid,
        focused: st.az.view.focused == Some(id),
        geometry: aizome_ipc::Rect::new(r.x, r.y, r.w, r.h),
        desk: m.view.desk,
        minimised: m.view.minimised,
        hidden: m.view.hidden,
        ..Default::default()
    })
}

use smithay::reexports::wayland_server::Resource;

/// The scene without a logic: popups, layer surfaces, and windows where
/// they last were, bare.
pub fn scene(st: &mut State, output: &Output, scale: f64, out: &mut Scene) {
    let geo = st.az.output_geometry(output);
    let origin = (geo.loc.x as f64, geo.loc.y as f64);
    crate::render::popup_items(&st.az, output, scale, false, out);
    crate::render::layer_items(output, Layer::Overlay, scale, out);
    crate::render::layer_items(output, Layer::Top, scale, out);
    for id in st.az.view.stacking.iter().rev() {
        let Some(m) = st.az.windows.get(id) else { continue };
        if !m.view.shown {
            continue;
        }
        let r = m.view.rect;
        if let Some(item) = crate::render::window_item(&st.az, *id, (r.x as f64, r.y as f64), origin, scale, 1.0) {
            out.push(item);
        }
    }
    crate::render::layer_items(output, Layer::Bottom, scale, out);
    crate::render::layer_items(output, Layer::Background, scale, out);
}

/// The client surface under a point: the lock while locked, else popups,
/// overlay and top layers, windows (where the logic last put them), bottom
/// and background layers.
pub fn surface_at(st: &mut State, pos: Point<f64, Logical>) -> Option<SurfaceAt> {
    let output = st.az.output_at(pos)?;
    if st.az.lock.is_locked() {
        return crate::input::lock_under(&st.az, &output, pos);
    }
    let out_loc = st.az.output_geometry(&output).loc.to_f64();
    if let Some(u) = st.az.popup_under(pos) {
        return Some(u);
    }
    if let Some(u) = crate::input::layer_under(&output, &[Layer::Overlay, Layer::Top], pos, out_loc) {
        return Some(u);
    }
    for id in st.az.view.stacking.iter().rev() {
        let Some(m) = st.az.windows.get(id) else { continue };
        if !m.view.shown {
            continue;
        }
        let geo = m.window.geometry();
        let origin =
            Point::<f64, Logical>::from(((m.view.rect.x - geo.loc.x) as f64, (m.view.rect.y - geo.loc.y) as f64));
        if let Some((s, loc)) =
            m.window.surface_under(pos - origin, WindowSurfaceType::TOPLEVEL | WindowSurfaceType::SUBSURFACE)
        {
            return Some((Focus(s), loc.to_f64() + origin, Owner::Window(*id)));
        }
    }
    crate::input::layer_under(&output, &[Layer::Bottom, Layer::Background], pos, out_loc)
}

pub fn pointer_motion(st: &mut State, pos: Point<f64, Logical>, time: u32) {
    let Some(pointer) = st.az.seat.get_pointer() else { return };
    let under = surface_at(st, pos);
    if !pointer.is_grabbed() {
        st.az.cursor_override = if under.is_none() { Some(smithay::input::pointer::CursorIcon::Default) } else { None };
    }
    let focus = under.map(|(f, o, _)| (f, o));
    pointer.motion(st, focus, &MotionEvent { location: pos, serial: SERIAL_COUNTER.next_serial(), time });
    pointer.frame(st);
}

pub fn pointer_button(st: &mut State, button: u32, pressed: bool, time: u32) {
    let Some(pointer) = st.az.seat.get_pointer() else { return };
    let serial = SERIAL_COUNTER.next_serial();
    if pressed {
        let pos = pointer.current_location();
        let under = surface_at(st, pos);
        st.press_outside_menus(under.as_ref().map(|u| &u.0.0), serial, time);
        if !pointer.is_grabbed() {
            focus_under(st, under);
        }
    }
    st.forward_button(button, pressed, serial, time);
}

pub fn pointer_axis(st: &mut State, a: &AxisEvent) {
    let Some(pointer) = st.az.seat.get_pointer() else { return };
    let mut frame = AxisFrame::new(a.time).source(a.source);
    use smithay::backend::input::Axis;
    if a.h != 0.0 {
        frame = frame.value(Axis::Horizontal, a.h);
    }
    if a.v != 0.0 {
        frame = frame.value(Axis::Vertical, a.v);
    }
    pointer.axis(st, frame);
    pointer.frame(st);
}

/// A touch or click focuses what's under it.
pub fn focus_at(st: &mut State, pos: Point<f64, Logical>) {
    let under = surface_at(st, pos);
    focus_under(st, under);
}

fn focus_under(st: &mut State, under: Option<SurfaceAt>) {
    match under {
        Some((_, _, Owner::Window(id))) => {
            if st.az.view.focused != Some(id) || st.az.view.stacking.last() != Some(&id) {
                focus(st, Some(id));
            }
        }
        Some((f, _, Owner::Layer)) => {
            let interactive = st.az.outputs.iter().find_map(|o| {
                layer_map_for_output(o)
                    .layer_for_surface(&f.0, WindowSurfaceType::ALL)
                    .map(|l| l.cached_state().keyboard_interactivity)
            });
            if interactive == Some(KeyboardInteractivity::OnDemand) {
                st.az.layer_focus = Some(f.0.clone());
                st.update_keyboard_focus();
            }
        }
        _ => {}
    }
}
