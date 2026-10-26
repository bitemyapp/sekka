//! In the Full effects tier the shell draws its menus, the Deskbar, the
//! palette and the notification cards frosted: their faces 90% opaque
//! (94% in Day) with a fine grain. Under GLES the frame blurs what's
//! behind them too, so what shows through is soft rather than sharp
//! (`aizome_comp_core::blur`); this decides which surfaces get it: the
//! shell's frosted layer surfaces, and every popup (menu) of the shell's
//! surfaces. Pixman keeps the frost as the shell draws it.

use aizome_comp_core::aizome_theme::Effects;
use aizome_comp_core::scene::{Scene, SceneItem};
use smithay::desktop::{WindowSurfaceType, find_popup_root_surface, layer_map_for_output};
use smithay::output::Output;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;

use crate::prelude::*;

/// The shell's layer surfaces drawn frosted in Full
/// (`crates/aizome-shell`: the Deskbar, torn-off menus, the palette and
/// the notification cards).
pub const FROSTED: [&str; 5] =
    ["aizome-deskbar", "aizome-deskbar-transient", "aizome-menu", "aizome-palette", crate::slides::NOTES];

/// Whose popups (menus) are blurred behind: the shell's.
const SHELL: &str = "aizome-";

/// Puts a blur straight behind each frosted surface in a scene.
pub fn behind(st: &State, output: &Output, scene: &mut Scene) {
    if st.az.settings.appearance.effects != Effects::Full || st.backend.software() {
        return;
    }
    let name = output.name();
    let mut i = 0;
    while i < scene.len() {
        if let SceneItem::Surface { surface, loc, .. } = &scene[i]
            && frosted(st, surface)
        {
            let blur = SceneItem::Blur { surface: surface.clone(), loc: *loc, output: name.clone() };
            scene.insert(i + 1, blur);
            i += 1;
        }
        i += 1;
    }
}

/// The namespace of the layer surface `surface` is, on any output.
fn namespace(st: &State, surface: &WlSurface) -> Option<String> {
    st.az.outputs.iter().find_map(|o| {
        layer_map_for_output(o)
            .layer_for_surface(surface, WindowSurfaceType::TOPLEVEL)
            .map(|l| l.namespace().to_owned())
    })
}

fn frosted(st: &State, surface: &WlSurface) -> bool {
    if let Some(ns) = namespace(st, surface) {
        return FROSTED.contains(&ns.as_str());
    }
    let Some(popup) = st.az.popups.find_popup(surface) else { return false };
    let Ok(root) = find_popup_root_surface(&popup) else { return false };
    namespace(st, &root).is_some_and(|ns| ns.starts_with(SHELL))
}
