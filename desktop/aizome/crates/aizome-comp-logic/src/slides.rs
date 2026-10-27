//! The shell's notification cards slide in from the screen's right edge
//! when they appear and slide back out when they go.
//!
//! The cards are one layer surface of the shell's (namespace
//! [`NOTES`], top layer, anchored to the top right beside the Deskbar). The
//! design puts them at the top right and says how motion behaves (it moves
//! layers, never longer than 150 ms, none with reduced motion) but draws no
//! entrance of its own, so this is the compositor's motion: the effects
//! tier's length (120 ms Full, 80 ms Lean, none in Austere or with reduced
//! motion) on its cubic ease-out, with no fade. A card going plays its
//! entrance backwards from its last frame, which the core keeps for us
//! ([`aizome_comp_core::ghosts`]).
//!
//! Only what's drawn moves: the layer map, and so where the pointer finds
//! the cards, has them in their place throughout. While they move they're
//! drawn under the other top-layer surfaces, so with the Deskbar showing
//! they come out from under it.
//!
//! What's moving is kept across a reload of the logic ([`Saved`]), and
//! cards already up when a logic starts don't come in again.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use aizome_comp_core::render::layer_items;
use aizome_comp_core::scene::{Scene, SceneItem};
use serde::{Deserialize, Serialize};
use smithay::backend::renderer::element::Kind;
use smithay::backend::renderer::utils::with_renderer_surface_state;
use smithay::desktop::layer_map_for_output;
use smithay::output::Output;
use smithay::reexports::wayland_server::Resource;
use smithay::reexports::wayland_server::backend::ObjectId;
use smithay::utils::{Logical, Physical, Point};
use smithay::wayland::shell::wlr_layer::Layer;

use crate::anim::ease_out;
use crate::prelude::*;

/// The namespace of the shell's notification cards
/// (`crates/aizome-shell/src/notifications/cards.rs`).
pub const NOTES: &str = "aizome-notifications";

/// A card coming in (its surface still there) or going (its last frame).
#[derive(Clone, Debug)]
pub struct Slide {
    pub output: String,
    /// How far right of its place it starts (coming in) or ends (going),
    /// in points.
    pub dx: f64,
    pub start: Instant,
    pub dur: Duration,
}

impl Slide {
    fn t(&self) -> f64 {
        self.start.elapsed().as_secs_f64() / self.dur.as_secs_f64().max(1e-6)
    }

    pub fn done(&self) -> bool {
        self.start.elapsed() >= self.dur
    }

    /// Coming in: what's left of the way, eased out.
    fn offset_in(&self) -> f64 {
        self.dx * (1.0 - ease_out(self.t()))
    }

    /// Going: the entrance backwards.
    fn offset_out(&self) -> f64 {
        self.dx * (1.0 - ease_out(1.0 - self.t()))
    }
}

/// Cards in motion.
#[derive(Debug, Default)]
pub struct Slides {
    /// Surfaces coming in, by surface.
    pub coming: HashMap<ObjectId, Slide>,
    /// Last frames going out, by the core's ghost id.
    pub going: HashMap<u64, Slide>,
    /// Card surfaces seen up (so they come in once).
    pub seen: HashSet<ObjectId>,
}

impl Slides {
    pub fn active(&self) -> bool {
        !self.coming.is_empty() || !self.going.is_empty()
    }
}

/// A slide as a reload carries it.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SavedSlide {
    pub output: String,
    /// The card surface's protocol id (coming in), or none.
    pub surface: Option<u32>,
    /// The core's ghost (going out), or none.
    pub ghost: Option<u64>,
    pub dx: f64,
    pub elapsed_ms: u64,
    pub dur_ms: u64,
}

/// Whether a layer surface has a buffer up.
fn shows(surface: &smithay::reexports::wayland_server::protocol::wl_surface::WlSurface) -> bool {
    with_renderer_surface_state(surface, |s| s.buffer().is_some()).unwrap_or(false)
}

/// What a reload carries.
pub fn save(l: &Logic) -> Vec<SavedSlide> {
    let ms = |s: &Slide| (s.start.elapsed().as_millis() as u64, s.dur.as_millis() as u64);
    let mut out = Vec::new();
    for (id, s) in &l.slides.coming {
        let (elapsed_ms, dur_ms) = ms(s);
        out.push(SavedSlide {
            output: s.output.clone(),
            surface: Some(id.protocol_id()),
            ghost: None,
            dx: s.dx,
            elapsed_ms,
            dur_ms,
        });
    }
    for (id, s) in &l.slides.going {
        let (elapsed_ms, dur_ms) = ms(s);
        out.push(SavedSlide {
            output: s.output.clone(),
            surface: None,
            ghost: Some(*id),
            dx: s.dx,
            elapsed_ms,
            dur_ms,
        });
    }
    out
}

/// A new logic: every card up now has been seen, and what was moving
/// goes on moving.
pub fn restore(st: &mut State, saved: &[SavedSlide]) {
    let now = Instant::now();
    let mut seen = HashSet::new();
    let mut coming = HashMap::new();
    let mut going = HashMap::new();
    for o in st.az.outputs.clone() {
        let map = layer_map_for_output(&o);
        for layer in map.layers().filter(|l| l.namespace() == NOTES) {
            let surface = layer.wl_surface();
            if !shows(surface) {
                continue;
            }
            seen.insert(surface.id());
            let name = o.name();
            if let Some(s) = saved.iter().find(|s| s.output == name && s.surface == Some(surface.id().protocol_id())) {
                coming.insert(surface.id(), carried(s, now));
            }
        }
    }
    for s in saved {
        if let Some(g) = s.ghost
            && st.az.ghosts.iter().any(|x| x.id == g)
        {
            going.insert(g, carried(s, now));
        }
    }
    st.lg.m().slides = Slides { coming, going, seen };
}

fn carried(s: &SavedSlide, now: Instant) -> Slide {
    let elapsed = Duration::from_millis(s.elapsed_ms);
    Slide {
        output: s.output.clone(),
        dx: s.dx,
        start: now.checked_sub(elapsed).unwrap_or(now),
        dur: Duration::from_millis(s.dur_ms),
    }
}

/// Starts cards that just came up moving in, and the last frames of
/// cards that went moving out; drops what's done.
fn track(st: &mut State, output: &Output) {
    let ms = st.az.motion_ms();
    // A restarted shell's cards replace the old shell's where they stand.
    let handover = st.az.children.handover.is_some();
    let out_w = st.az.output_geometry(output).size.w as f64;
    let name = output.name();
    let up: Vec<(ObjectId, f64)> = {
        let map = layer_map_for_output(output);
        map.layers()
            .filter(|l| l.namespace() == NOTES && shows(l.wl_surface()))
            .filter_map(|l| map.layer_geometry(l).map(|g| (l.wl_surface().id(), g.loc.x as f64)))
            .collect()
    };
    let all_up: HashSet<ObjectId> = st
        .az
        .outputs
        .iter()
        .flat_map(|o| {
            layer_map_for_output(o)
                .layers()
                .filter(|l| l.namespace() == NOTES && shows(l.wl_surface()))
                .map(|l| l.wl_surface().id())
                .collect::<Vec<_>>()
        })
        .collect();
    let l = st.lg.m();
    l.slides.coming.retain(|_, s| !s.done());
    for (id, x) in &up {
        if l.slides.seen.insert(id.clone()) && ms > 0 && !handover {
            let slide = Slide { output: name.clone(), dx: out_w - x, start: Instant::now(), dur: ms_dur(ms) };
            l.slides.coming.insert(id.clone(), slide);
        }
    }
    // Forget cards that have gone.
    l.slides.seen.retain(|id| all_up.contains(id));

    // Last frames: start them going, drop them once gone.
    let mut drop_ghosts = Vec::new();
    for g in st.az.ghosts.iter().filter(|g| &g.output == output && g.namespace == NOTES) {
        match l.slides.going.get(&g.id) {
            Some(s) if s.done() => drop_ghosts.push(g.id),
            Some(_) => {}
            None if ms == 0 || handover => drop_ghosts.push(g.id),
            None => {
                let dx = out_w - g.geometry.loc.x as f64;
                let slide = Slide { output: name.clone(), dx, start: Instant::now(), dur: ms_dur(ms) };
                l.slides.going.insert(g.id, slide);
            }
        }
    }
    for id in &drop_ghosts {
        l.slides.going.remove(id);
    }
    st.az.ghosts.retain(|g| !drop_ghosts.contains(&g.id));
    // Ghosts the core dropped meanwhile.
    let ghosts: HashSet<u64> = st.az.ghosts.iter().map(|g| g.id).collect();
    st.lg.m().slides.going.retain(|id, _| ghosts.contains(id));
}

fn ms_dur(ms: u32) -> Duration {
    Duration::from_millis(ms as u64)
}

/// The top layer, front first: its surfaces in place, then cards moving
/// in and cards moving out, under them.
pub fn top_layer(st: &mut State, output: &Output, scale: f64, out: &mut Scene) {
    track(st, output);
    let l = st.lg.r();
    if !l.slides.active() {
        layer_items(output, Layer::Top, scale, out);
        return;
    }
    let mut moving = Vec::new();
    {
        let map = layer_map_for_output(output);
        for layer in map.layers_on(Layer::Top).rev() {
            let Some(geo) = map.layer_geometry(layer) else { continue };
            let surface = layer.wl_surface();
            let dx = l.slides.coming.get(&surface.id()).map(|s| s.offset_in());
            let at = Point::<f64, Logical>::from((geo.loc.x as f64 + dx.unwrap_or(0.0), geo.loc.y as f64));
            let loc: Point<i32, Physical> = at.to_physical_precise_round(scale);
            let item = SceneItem::Surface { surface: surface.clone(), loc, alpha: 1.0, kind: Kind::Unspecified };
            if dx.is_some() { moving.push(item) } else { out.push(item) }
        }
    }
    out.extend(moving);
    for g in st.az.ghosts.iter().filter(|g| &g.output == output) {
        if let Some(s) = l.slides.going.get(&g.id) {
            out.push(g.item((s.offset_out(), 0.0), scale, 1.0));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slide(elapsed_ms: u64, dur_ms: u64) -> Slide {
        let now = Instant::now();
        Slide {
            output: "HEADLESS-1".into(),
            dx: 600.0,
            start: now - Duration::from_millis(elapsed_ms),
            dur: Duration::from_millis(dur_ms),
        }
    }

    #[test]
    fn cards_come_in_from_the_edge_and_go_back_out_to_it() {
        let start = slide(0, 10_000);
        assert!((start.offset_in() - 600.0).abs() < 1.0);
        assert!(start.offset_out().abs() < 1.0);
        let end = slide(10_000, 10_000);
        assert!(end.done());
        assert_eq!(end.offset_in(), 0.0);
        assert_eq!(end.offset_out(), 600.0);
        // Halfway in, eased out: most of the way there.
        let half = slide(5_000, 10_000);
        assert!(half.offset_in() < 100.0, "{}", half.offset_in());
        // Going is coming backwards.
        let back = slide(5_000, 10_000);
        assert!((back.offset_out() - half.offset_in()).abs() < 1.0);
    }

    #[test]
    fn a_reload_carries_how_far_a_slide_got() {
        let s = slide(40, 120);
        let saved = SavedSlide {
            output: s.output.clone(),
            surface: Some(7),
            ghost: None,
            dx: s.dx,
            elapsed_ms: s.start.elapsed().as_millis() as u64,
            dur_ms: 120,
        };
        let json = serde_json::to_string(&saved).unwrap();
        let back: SavedSlide = serde_json::from_str(&json).unwrap();
        let c = carried(&back, Instant::now());
        assert!(c.start.elapsed() >= Duration::from_millis(40));
        assert_eq!(c.dur, Duration::from_millis(120));
        // A state from before slides were saved reads as none.
        let old: crate::Saved = serde_json::from_str(r#"{"model":null,"requested_sizes":{}}"#).unwrap();
        assert!(old.slides.is_empty());
    }
}
