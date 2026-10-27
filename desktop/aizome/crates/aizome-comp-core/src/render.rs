//! Frames: the scene for an output (the pointer, the drag icon and the
//! lock screen from the core, everything else from the logic), and the
//! render elements smithay draws, for whichever renderer the backend has.
//!
//! Decorations are solid-colour elements with stable identities and
//! cached label textures, and window surfaces come straight from their
//! buffers, so the damage tracker redraws only what changed: moving a
//! window composites and asks nothing of its client.

use smithay::backend::renderer::element::Kind;
use smithay::backend::renderer::element::memory::MemoryRenderBufferRenderElement;
use smithay::backend::renderer::element::solid::SolidColorRenderElement;
use smithay::backend::renderer::element::surface::{WaylandSurfaceRenderElement, render_elements_from_surface_tree};
use smithay::backend::renderer::element::utils::{CropRenderElement, RelocateRenderElement, RescaleRenderElement};
use smithay::backend::renderer::{ImportAll, ImportMem, Renderer};
use smithay::desktop::layer_map_for_output;
use smithay::desktop::utils::bbox_from_surface_tree;
use smithay::input::pointer::{CursorImageStatus, CursorImageSurfaceData};
use smithay::output::Output;
use smithay::utils::{Logical, Physical, Point, Rectangle, Size};
use smithay::wayland::compositor::with_states;
use smithay::wayland::shell::wlr_layer::Layer;

use crate::WinId;
use crate::blur::BlurRenderer;
use crate::input::Owner;
use crate::scene::{Purpose, Scene, SceneItem};
use crate::state::{Aizome, State};

smithay::backend::renderer::element::render_elements! {
    pub OutputElement<R> where R: ImportAll + ImportMem + BlurRenderer;
    Surface = WaylandSurfaceRenderElement<R>,
    Solid = SolidColorRenderElement,
    Memory = MemoryRenderBufferRenderElement<R>,
    Thumb = CropRenderElement<RelocateRenderElement<RescaleRenderElement<WaylandSurfaceRenderElement<R>>>>,
    Blur = crate::blur::BlurElement,
}

/// The colour behind everything.
pub fn clear_color(az: &Aizome) -> [f32; 4] {
    if az.lock.is_locked() { [0.0, 0.0, 0.0, 1.0] } else { az.palette.desktop.to_premultiplied_f32() }
}

impl State {
    /// Everything an output shows, front first: the pointer and the drag
    /// icon, then the lock screen while locked, else the recovery overlay
    /// and the logic's scene (or, without a logic, the recovery path's).
    pub fn scene(&mut self, output: &Output, purpose: Purpose) -> Scene {
        self.expire_ghosts();
        let scale = output.current_scale().fractional_scale();
        let mut out: Scene = Vec::new();
        let cursor = match purpose {
            Purpose::Screen => true,
            Purpose::Capture { cursor } => cursor,
        };
        if cursor {
            cursor_items(&mut self.az, output, scale, &mut out);
        }
        // Ctrl+Alt+Del's overlay, above everything (the lock screen too).
        out.extend(crate::recovery::items(&mut self.az, output, scale));
        drag_icon_items(&self.az, output, scale, &mut out);
        if self.az.lock.is_locked() {
            if let Some(lock) = self.az.out.get(output).and_then(|o| o.lock_surface.clone()) {
                out.push(SceneItem::Surface {
                    surface: lock.wl_surface().clone(),
                    loc: (0, 0).into(),
                    alpha: 1.0,
                    kind: Kind::Unspecified,
                });
            }
            return out;
        }
        if self.lg.healthy() {
            let scene = self.lg_call(None, |vt, st| Some((vt.scene)(st, output, purpose)));
            if let Some(scene) = scene {
                out.extend(scene);
                return out;
            }
        }
        crate::fallback::scene(self, output, scale, &mut out);
        out
    }
}

/// Render elements for a scene at an output's scale.
pub fn scene_elements<R>(renderer: &mut R, scale: f64, scene: &Scene) -> Vec<OutputElement<R>>
where
    R: Renderer + ImportAll + ImportMem + crate::blur::BlurRenderer,
    R::TextureId: Clone + Send + 'static,
{
    let mut out: Vec<OutputElement<R>> = Vec::with_capacity(scene.len() + 16);
    for item in scene {
        match item {
            SceneItem::Surface { surface, loc, alpha, kind } => out.extend(
                render_elements_from_surface_tree(renderer, surface, *loc, scale, *alpha, *kind)
                    .into_iter()
                    .map(OutputElement::Surface),
            ),
            SceneItem::Solid(e) => out.push(OutputElement::Solid(e.clone())),
            SceneItem::Memory { buffer, loc, alpha, src, size, kind } => {
                if let Ok(e) =
                    MemoryRenderBufferRenderElement::from_buffer(renderer, *loc, buffer, *alpha, *src, *size, *kind)
                {
                    out.push(OutputElement::Memory(e));
                }
            }
            SceneItem::Thumb { surface, surface_loc, alpha, k, target, crop } => {
                let elems: Vec<WaylandSurfaceRenderElement<R>> = render_elements_from_surface_tree(
                    renderer,
                    surface,
                    *surface_loc,
                    scale,
                    *alpha,
                    Kind::Unspecified,
                );
                out.extend(elems.into_iter().filter_map(|e| {
                    let e = RescaleRenderElement::from_element(e, Point::from((0, 0)), *k);
                    let e = RelocateRenderElement::from_element(
                        e,
                        *target,
                        smithay::backend::renderer::element::utils::Relocate::Relative,
                    );
                    CropRenderElement::from_element(e, scale, *crop).map(OutputElement::Thumb)
                }));
            }
            SceneItem::Blur { surface, loc, output } => {
                out.extend(crate::blur::BlurElement::new(surface, *loc, output, scale).map(OutputElement::Blur));
            }
        }
    }
    out
}

/// Layer surfaces on one layer of an output (their popups are in the
/// popup layer), front first.
pub fn layer_items(output: &Output, layer: Layer, scale: f64, out: &mut Scene) {
    let map = layer_map_for_output(output);
    for surface in map.layers_on(layer).rev() {
        let Some(geo) = map.layer_geometry(surface) else { continue };
        let loc = geo.loc.to_physical_precise_round(scale);
        out.push(SceneItem::Surface {
            surface: surface.wl_surface().clone(),
            loc,
            alpha: 1.0,
            kind: Kind::Unspecified,
        });
    }
}

/// The popup layer: every window's and layer surface's popups, above them
/// all, newest in front. `skip_windows` leaves windows' popups out (they
/// would be in the wrong place while Desks slide).
pub fn popup_items(az: &Aizome, output: &Output, scale: f64, skip_windows: bool, out: &mut Scene) {
    let geo = az.output_geometry(output);
    for p in az.placed_popups().into_iter().rev() {
        if skip_windows && matches!(p.owner, Owner::Window(_)) {
            continue;
        }
        if !bbox_from_surface_tree(p.popup.wl_surface(), p.loc).overlaps(geo) {
            continue;
        }
        let loc = (p.loc - geo.loc).to_physical_precise_round(scale);
        out.push(SceneItem::Surface {
            surface: p.popup.wl_surface().clone(),
            loc,
            alpha: 1.0,
            kind: Kind::Unspecified,
        });
    }
}

/// A window's surfaces (its toplevel and subsurfaces; popups are in the
/// popup layer), with its content's top-left at `at` (global points).
pub fn window_item(
    az: &Aizome,
    id: WinId,
    at: (f64, f64),
    origin: (f64, f64),
    scale: f64,
    alpha: f32,
) -> Option<SceneItem> {
    let m = az.windows.get(&id)?;
    let geo = m.window.geometry();
    let loc = Point::<f64, Logical>::from((at.0 - geo.loc.x as f64 - origin.0, at.1 - geo.loc.y as f64 - origin.1));
    let loc: Point<i32, Physical> = loc.to_physical_precise_round(scale);
    Some(SceneItem::Surface { surface: m.surface().clone(), loc, alpha, kind: Kind::Unspecified })
}

/// A window's surfaces scaled into a box (overview and switcher
/// thumbnails): the client's current buffers, scaled at composition, never
/// redrawn by the client.
pub fn thumbnail_item(
    az: &Aizome,
    id: WinId,
    dst: (f64, f64, f64, f64),
    origin: (f64, f64),
    scale: f64,
    alpha: f32,
) -> Option<SceneItem> {
    let m = az.windows.get(&id)?;
    let geo = m.window.geometry();
    if geo.size.w <= 0 || geo.size.h <= 0 {
        return None;
    }
    let k = (dst.2 / geo.size.w as f64).min(dst.3 / geo.size.h as f64);
    // Render the surface at its origin, rescale about the origin, then move
    // to the box (centred), cropped to it.
    let surface_loc: Point<i32, Physical> =
        Point::<f64, Logical>::from((-geo.loc.x as f64, -geo.loc.y as f64)).to_physical_precise_round(scale);
    let w = geo.size.w as f64 * k;
    let h = geo.size.h as f64 * k;
    let x = dst.0 + (dst.2 - w) / 2.0;
    let y = dst.1 + (dst.3 - h) / 2.0;
    let target: Point<i32, Physical> =
        Point::<f64, Logical>::from((x - origin.0, y - origin.1)).to_physical_precise_round(scale);
    let crop: Rectangle<i32, Physical> =
        Rectangle::new(target, Size::<f64, Logical>::from((w, h)).to_physical_precise_round(scale));
    Some(SceneItem::Thumb { surface: m.surface().clone(), surface_loc, alpha, k, target, crop })
}

fn cursor_items(az: &mut Aizome, output: &Output, scale: f64, out: &mut Scene) {
    let geo = az.output_geometry(output);
    let pos = az.pointer_pos;
    if !geo.to_f64().contains(pos) {
        return;
    }
    let rel = pos - geo.loc.to_f64();
    let named = match (&az.cursor_override, &az.cursor_status) {
        (Some(icon), _) => Some(icon.name()),
        (None, CursorImageStatus::Hidden) => return,
        (None, CursorImageStatus::Named(icon)) => Some(icon.name()),
        (None, CursorImageStatus::Surface(_)) => None,
    };
    match named {
        Some(name) => {
            let img = az.cursor_theme.get(name, scale.ceil().max(1.0) as u32);
            let loc: Point<i32, Physical> =
                Point::<f64, Logical>::from((rel.x - img.hot.0, rel.y - img.hot.1)).to_physical_precise_round(scale);
            let size = Size::from((img.size.0.round() as i32, img.size.1.round() as i32));
            out.push(SceneItem::Memory {
                buffer: img.buffer.clone(),
                loc: loc.to_f64(),
                alpha: None,
                src: None,
                size: Some(size),
                kind: Kind::Cursor,
            });
        }
        None => {
            let CursorImageStatus::Surface(surface) = &az.cursor_status else { return };
            let hotspot = with_states(surface, |states| {
                states.data_map.get::<CursorImageSurfaceData>().map(|d| d.lock().unwrap().hotspot).unwrap_or_default()
            });
            let loc: Point<i32, Physical> = (rel - hotspot.to_f64()).to_physical_precise_round(scale);
            out.push(SceneItem::Surface { surface: surface.clone(), loc, alpha: 1.0, kind: Kind::Cursor });
        }
    }
}

/// A drag-and-drop icon: at the pointer, moved by its offsets, above
/// everything but the cursor (and in screenshots, as a window would be).
fn drag_icon_items(az: &Aizome, output: &Output, scale: f64, out: &mut Scene) {
    let Some(icon) = az.dnd_icon.clone() else { return };
    let geo = az.output_geometry(output);
    let at = az.pointer_pos + az.dnd_offset.to_f64();
    let bbox = smithay::desktop::utils::bbox_from_surface_tree(&icon, at.to_i32_round());
    if !bbox.overlaps(geo) {
        return;
    }
    let loc: Point<i32, Physical> = (at - geo.loc.to_f64()).to_physical_precise_round(scale);
    out.push(SceneItem::Surface { surface: icon, loc, alpha: 1.0, kind: Kind::Unspecified });
}
