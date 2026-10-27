//! Every surface's preferred scale and transform, kept current.
//!
//! A client learns the scale to draw at from wp_fractional_scale (its
//! preferred scale) and from wl_surface's preferred_buffer_scale and
//! preferred_buffer_transform (wl_surface 6). The core says so once, when
//! a client asks for its fractional scale; nothing said it again when an
//! output's scale or transform changed or a window moved to another
//! output. Every app already running then kept drawing at the old scale
//! and the compositor stretched it: a 4K display switched to 2× in
//! Settings showed every open app blurred until it was restarted.
//!
//! Here each window (its subsurfaces and popups too), each layer surface
//! and each lock surface is told the scale and transform of the output it
//! shows on, again whenever those change. smithay sends an event only when
//! the value differs from the last one sent, so telling a surface what it
//! already knows costs nothing on the wire.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use smithay::desktop::{PopupManager, layer_map_for_output};
use smithay::output::Output;
use smithay::reexports::wayland_server::Resource;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::utils::Transform;
use smithay::wayland::compositor::{TraversalAction, send_surface_state, with_surface_tree_downward};
use smithay::wayland::fractional_scale::with_fractional_scale;

use crate::prelude::*;

/// What a surface is told: its output's scale and transform.
#[derive(Clone, Copy, PartialEq)]
struct Look {
    scale: f64,
    transform: Transform,
}

impl Look {
    fn of(output: &Output) -> Look {
        Look { scale: output.current_scale().fractional_scale(), transform: output.current_transform() }
    }
}

/// Tells every surface that needs it. Cheap when nothing changed: a hash
/// of the outputs' scales and which surfaces show where.
pub fn refresh(st: &mut State) {
    let (sig, plan) = plan(st);
    if st.lg.r().scales_seen == Some(sig) {
        return;
    }
    for (surface, look) in &plan {
        tell(surface, *look);
        for (popup, _) in PopupManager::popups_for_surface(surface) {
            tell(popup.wl_surface(), *look);
        }
    }
    st.lg.m().scales_seen = Some(sig);
}

/// The surfaces to tell and what, and a signature of it all.
fn plan(st: &State) -> (u64, Vec<(WlSurface, Look)>) {
    let az = &st.az;
    let mut h = DefaultHasher::new();
    let mut plan = Vec::new();
    for o in &az.outputs {
        o.name().hash(&mut h);
        o.current_scale().fractional_scale().to_bits().hash(&mut h);
        (o.current_transform() as u32).hash(&mut h);
    }
    // Windows: the output showing most of them; a window off every output
    // (another Desk, minimised) keeps the active one's.
    let fallback = az.active_output().or_else(|| az.outputs.first().cloned());
    let mut ids: Vec<WinId> = az.windows.keys().copied().collect();
    ids.sort();
    for id in ids {
        let Some(m) = az.windows.get(&id) else { continue };
        let output = az.window_rect(id).and_then(|r| az.output_for_rect(r)).or_else(|| fallback.clone());
        let Some(output) = output else { continue };
        id.hash(&mut h);
        output.name().hash(&mut h);
        popup_count(m.surface()).hash(&mut h);
        plan.push((m.surface().clone(), Look::of(&output)));
    }
    // Toplevels still to draw for the first time go where new windows go.
    if let Some(o) = &fallback {
        for u in &az.unmapped {
            if let Some(t) = u.window.toplevel() {
                t.wl_surface().id().protocol_id().hash(&mut h);
                plan.push((t.wl_surface().clone(), Look::of(o)));
            }
        }
    }
    // The shell's and other layer surfaces, and lock surfaces, on their
    // own output.
    for o in &az.outputs {
        let look = Look::of(o);
        for layer in layer_map_for_output(o).layers() {
            let s = layer.wl_surface();
            s.id().protocol_id().hash(&mut h);
            popup_count(s).hash(&mut h);
            plan.push((s.clone(), look));
        }
        if let Some(lock) = az.out.get(o).and_then(|os| os.lock_surface.as_ref()) {
            lock.wl_surface().id().protocol_id().hash(&mut h);
            plan.push((lock.wl_surface().clone(), look));
        }
    }
    (h.finish(), plan)
}

fn popup_count(surface: &WlSurface) -> usize {
    PopupManager::popups_for_surface(surface).count()
}

/// One surface and its subsurfaces.
fn tell(surface: &WlSurface, look: Look) {
    let integer = look.scale.ceil().max(1.0) as i32;
    with_surface_tree_downward(
        surface,
        (),
        |_, _, _| TraversalAction::DoChildren(()),
        |s, states, _| {
            with_fractional_scale(states, |fs| fs.set_preferred_scale(look.scale));
            send_surface_state(s, states, integer, look.transform);
        },
        |_, _, _| true,
    );
}
