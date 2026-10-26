//! A frame's contents, as the logic describes them: renderer-free, so the
//! logic needs no code per renderer and the core turns the same scene into
//! GLES, pixman or multi-GPU render elements (`render.rs`).
//!
//! Items are front first, as smithay's damage tracker takes elements.
//! Identities carry over: a solid colour keeps its element id and commit
//! counter, a memory buffer is the same buffer, and a surface is its
//! wl_surface, so an unchanged scene causes no damage.

use smithay::backend::renderer::element::Kind;
use smithay::backend::renderer::element::memory::MemoryRenderBuffer;
use smithay::backend::renderer::element::solid::SolidColorRenderElement;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::utils::{Logical, Physical, Point, Rectangle, Size};

/// Who a frame is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Purpose {
    /// The display itself.
    Screen,
    /// A screenshot or screen capture, with or without the pointer.
    Capture { cursor: bool },
}

/// One thing a frame shows.
#[derive(Clone, Debug)]
pub enum SceneItem {
    /// A surface tree (a window, a layer surface, a popup), its origin at
    /// `loc` on the output.
    Surface { surface: WlSurface, loc: Point<i32, Physical>, alpha: f32, kind: Kind },
    /// A flat colour with a stable identity.
    Solid(SolidColorRenderElement),
    /// Pixels in memory (a label, an icon, a cursor).
    Memory {
        buffer: MemoryRenderBuffer,
        loc: Point<f64, Physical>,
        alpha: Option<f32>,
        src: Option<Rectangle<f64, Logical>>,
        size: Option<Size<i32, Logical>>,
        kind: Kind,
    },
    /// A window's surfaces scaled into a box (thumbnails): drawn at
    /// `surface_loc`, scaled by `k` about the origin, moved to `target` and
    /// cropped to `crop`.
    Thumb {
        surface: WlSurface,
        surface_loc: Point<i32, Physical>,
        alpha: f32,
        k: f64,
        target: Point<i32, Physical>,
        crop: Rectangle<i32, Physical>,
    },
    /// A blur of what's behind a surface, under the surface's face (put
    /// it straight behind the surface's own item: [`crate::blur`]). GLES
    /// only; nothing under pixman.
    Blur { surface: WlSurface, loc: Point<i32, Physical>, output: String },
}

/// Scene items, front first.
pub type Scene = Vec<SceneItem>;
