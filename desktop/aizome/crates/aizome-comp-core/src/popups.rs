//! The popup layer: menus and other xdg_popups, of windows and layer
//! surfaces alike, show above every window and layer surface, newest on
//! top, anywhere on the output, and take input there first.

use smithay::desktop::utils::{bbox_from_surface_tree, under_from_surface_tree};
use smithay::desktop::{PopupKind, PopupManager, WindowSurfaceType, find_popup_root_surface, layer_map_for_output};
use smithay::output::Output;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::utils::{IsAlive, Logical, Point, Rectangle};

use crate::focus::Focus;
use crate::input::{Owner, SurfaceAt};
use crate::state::Aizome;

/// A popup that shows, and where its surface's origin is, in global points.
pub struct Placed {
    pub popup: PopupKind,
    pub loc: Point<i32, Logical>,
    pub owner: Owner,
}

impl Aizome {
    /// Where a popup root's positioner coordinates start (a window's
    /// geometry, a layer surface's origin), in global points; the output it
    /// is on; and its owner. `None` while the root doesn't show.
    pub fn popup_root(&self, root: &WlSurface) -> Option<(Point<i32, Logical>, Option<Output>, Owner)> {
        if let Some(id) = self.window_for_surface(root) {
            let w = &self.windows.get(&id)?.view;
            if !w.shown {
                return None;
            }
            return Some(((w.rect.x, w.rect.y).into(), self.output_for_rect(w.rect), Owner::Window(id)));
        }
        for o in &self.outputs {
            let map = layer_map_for_output(o);
            if let Some(l) = map.layer_for_surface(root, WindowSurfaceType::TOPLEVEL) {
                if !crate::focus::layer_mapped(l) {
                    return None;
                }
                let g = map.layer_geometry(l)?;
                return Some((g.loc + self.output_geometry(o).loc, Some(o.clone()), Owner::Layer));
            }
        }
        None
    }

    /// The popups that show, in the order they opened (oldest first).
    pub fn placed_popups(&self) -> Vec<Placed> {
        let mut out = Vec::new();
        for popup in &self.popup_order {
            if !popup.alive() {
                continue;
            }
            let Ok(root) = find_popup_root_surface(popup) else { continue };
            let Some((origin, _, owner)) = self.popup_root(&root) else { continue };
            // Only mapped popups are in their root's tree.
            let Some(offset) = PopupManager::popups_for_surface(&root).find(|(p, _)| p == popup).map(|(_, o)| o) else {
                continue;
            };
            out.push(Placed { loc: origin + offset - popup.geometry().loc, popup: popup.clone(), owner });
        }
        out
    }

    /// The popups' area, in global points, for redraws.
    pub fn popup_bbox(&self, popup: &PopupKind) -> Option<Rectangle<i32, Logical>> {
        self.placed_popups()
            .into_iter()
            .find(|p| &p.popup == popup)
            .map(|p| bbox_from_surface_tree(p.popup.wl_surface(), p.loc))
    }

    /// The popup (or a subsurface of one) under a point, topmost first.
    pub fn popup_under(&self, pos: Point<f64, Logical>) -> Option<SurfaceAt> {
        for p in self.placed_popups().into_iter().rev() {
            if let Some((s, loc)) = under_from_surface_tree(p.popup.wl_surface(), pos, p.loc, WindowSurfaceType::ALL) {
                return Some((Focus(s), loc.to_f64(), p.owner));
            }
        }
        None
    }

    /// Forgets popups that are gone.
    pub fn prune_popups(&mut self) {
        self.popup_order.retain(|p| p.alive());
    }
}
