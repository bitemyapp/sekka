//! Layer surfaces' last frames, kept for a moment after they go, so the
//! logic can move them away rather than have them vanish (the shell's
//! notification cards slide out).
//!
//! The logic names the namespaces it wants kept in its view
//! ([`crate::view::View::keep_on_unmap`]); when such a layer surface is
//! destroyed, the core draws it into memory first (its buffer is still
//! there: a client gives up the layer role before its surface) and keeps
//! the picture here, where the logic's scene finds it. The logic drops a
//! ghost when it's done with it; the core drops any older than a second.

use std::time::{Duration, Instant};

use smithay::backend::allocator::Fourcc;
use smithay::backend::renderer::element::Kind;
use smithay::backend::renderer::element::memory::MemoryRenderBuffer;
use smithay::backend::renderer::utils::with_renderer_surface_state;
use smithay::desktop::{LayerSurface, WindowSurfaceType};
use smithay::output::Output;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::utils::{Logical, Physical, Rectangle, Size, Transform};
use smithay::wayland::compositor::{BufferAssignment, SurfaceAttributes, with_states};
use smithay::wayland::shell::wlr_layer::Layer;

use crate::scene::SceneItem;
use crate::state::State;

/// The longest a ghost is kept.
const MAX_AGE: Duration = Duration::from_secs(1);

/// A layer surface's last frame.
#[derive(Clone, Debug)]
pub struct Ghost {
    /// Unique for the session (kept across reloads of the logic).
    pub id: u64,
    pub output: Output,
    pub namespace: String,
    pub layer: Layer,
    /// Where it was on its output, in points.
    pub geometry: Rectangle<i32, Logical>,
    /// Its pixels, at the output's scale.
    pub buffer: MemoryRenderBuffer,
    pub born: Instant,
}

impl Ghost {
    /// The ghost as a scene item, moved by `offset` points.
    pub fn item(&self, offset: (f64, f64), scale: f64, alpha: f32) -> SceneItem {
        let loc = smithay::utils::Point::<f64, Logical>::from((
            self.geometry.loc.x as f64 + offset.0,
            self.geometry.loc.y as f64 + offset.1,
        ))
        .to_physical_precise_round(scale);
        let loc: smithay::utils::Point<i32, Physical> = loc;
        SceneItem::Memory {
            buffer: self.buffer.clone(),
            loc: loc.to_f64(),
            alpha: Some(alpha),
            src: None,
            size: None,
            kind: Kind::Unspecified,
        }
    }
}

impl State {
    /// Keeps a layer surface's last frame, if the logic asked for its
    /// namespace (call before unmapping it).
    pub fn keep_ghost(&mut self, output: &Output, layer: &LayerSurface) {
        if !self.az.view.keep_on_unmap.iter().any(|n| n == layer.namespace()) {
            return;
        }
        // Taken down already (a client unmaps before it destroys).
        if !with_renderer_surface_state(layer.wl_surface(), |s| s.buffer().is_some()).unwrap_or(false) {
            return;
        }
        let map = smithay::desktop::layer_map_for_output(output);
        let Some(geometry) = map.layer_geometry(layer) else { return };
        drop(map);
        let scale = output.current_scale().fractional_scale();
        // Whole points at integer scales, as a memory buffer's size is.
        let s = scale.round().max(1.0) as i32;
        if (scale - s as f64).abs() > f64::EPSILON || geometry.size.w <= 0 || geometry.size.h <= 0 {
            return;
        }
        let size: Size<i32, Physical> = (geometry.size.w * s, geometry.size.h * s).into();
        let started = Instant::now();
        let pixels = match self.backend.render_to_memory(layer.wl_surface(), size, scale) {
            Ok(p) => p,
            Err(e) => {
                tracing::debug!("keeping {}'s last frame: {e}", layer.namespace());
                return;
            }
        };
        let buffer = MemoryRenderBuffer::from_slice(
            &pixels.rgba,
            Fourcc::Abgr8888,
            (pixels.width as i32, pixels.height as i32),
            s,
            Transform::Normal,
            None,
        );
        self.az.next_ghost += 1;
        self.az.ghosts.push(Ghost {
            id: self.az.next_ghost,
            output: output.clone(),
            namespace: layer.namespace().to_owned(),
            layer: layer.layer(),
            geometry,
            buffer,
            born: Instant::now(),
        });
        tracing::debug!(namespace = layer.namespace(), took = ?started.elapsed(), "kept a layer surface's last frame");
    }

    /// Keeps a layer surface's last frame when the commit being handled
    /// takes its buffer down (a null buffer: unmapped), if the logic asked
    /// for its namespace. Call before the commit lets the old buffer go.
    pub fn ghost_if_unmapping(&mut self, surface: &WlSurface) {
        if self.az.view.keep_on_unmap.is_empty() || self.shell_restart_holds_layout() {
            return;
        }
        let removed = with_states(surface, |s| {
            matches!(s.cached_state.get::<SurfaceAttributes>().current().buffer, Some(BufferAssignment::Removed))
        });
        if !removed {
            return;
        }
        let found = self.az.outputs.iter().find_map(|o| {
            let map = smithay::desktop::layer_map_for_output(o);
            map.layer_for_surface(surface, WindowSurfaceType::TOPLEVEL).cloned().map(|l| (o.clone(), l))
        });
        if let Some((output, layer)) = found {
            self.keep_ghost(&output, &layer);
        }
    }

    /// A layer surface is being destroyed: keep its last frame if it still
    /// shows one.
    pub fn ghost_on_destroy(&mut self, surface: &WlSurface) {
        if self.az.view.keep_on_unmap.is_empty() || self.shell_restart_holds_layout() {
            return;
        }
        let found = self.az.outputs.iter().find_map(|o| {
            let map = smithay::desktop::layer_map_for_output(o);
            map.layer_for_surface(surface, WindowSurfaceType::TOPLEVEL).cloned().map(|l| (o.clone(), l))
        });
        if let Some((output, layer)) = found {
            self.keep_ghost(&output, &layer);
        }
    }

    /// Forgets ghosts the logic didn't (it has gone, or forgot).
    pub fn expire_ghosts(&mut self) {
        self.az.ghosts.retain(|g| g.born.elapsed() < MAX_AGE);
    }
}
