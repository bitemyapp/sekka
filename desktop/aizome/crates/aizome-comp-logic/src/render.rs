//! What an output shows, front to back: overlays (marking menu, switcher,
//! overview), the popup layer, overlay-layer surfaces, a focused
//! full-screen window, top-layer surfaces, the tab-drag feedback, windows
//! with their tabs and frames, then bottom and background layers over the
//! desktop colour. The core puts the pointer and the drag icon in front,
//! and draws the lock screen instead of all this while locked.
//!
//! Decorations are solid colours with stable identities and cached label
//! textures, and window surfaces come straight from their buffers, so the
//! damage tracker redraws only what changed: moving a window composites
//! and asks nothing of its client.

use aizome_comp_core::render::{layer_items, popup_items, window_item};
use aizome_comp_core::scene::{Purpose, Scene, SceneItem};
use aizome_theme::typeface;
use smithay::backend::renderer::element::Kind;
use smithay::output::Output;
use smithay::utils::{Logical, Physical, Point, Size};
use smithay::wayland::shell::wlr_layer::Layer;

use crate::decor::{self, Layout, TabLook};
use crate::prelude::*;
use aizome_comp_core::text::{Face, Labels, Style};

/// Everything an output shows under the pointer, front first.
pub fn scene(st: &mut State, output: &Output, _purpose: Purpose) -> Scene {
    let scale = output.current_scale().fractional_scale();
    let geo = st.az.output_geometry(output);
    let origin = (geo.loc.x as f64, geo.loc.y as f64);
    let mut out: Scene = Vec::new();

    out.extend(crate::overlays::items(st, output, scale));
    if st.lg.r().overlays.covers_desktop(output) {
        // The overview hides the desktop entirely.
        return out;
    }

    // Windows' popups would be in the wrong place while Desks slide.
    let name = output.name();
    let sliding = st.lg.r().anims.desk_on(&name).is_some();
    popup_items(&st.az, output, scale, sliding, &mut out);
    layer_items(output, Layer::Overlay, scale, &mut out);

    let l = st.lg.r();
    let shown = l.wm.shown_stacking();
    let desk_slide = l.anims.desk_on(&name).cloned();
    let out_w = geo.size.w as f64;
    // A full-screen window in front on this display covers the top layer
    // too: the focused one, or the one focused last on a Desk another
    // display shows.
    let here = |id: &WinId| l.wm.screens().is_empty() || l.wm.screen_of(*id).is_some_and(|s| s.output == name);
    let top_fullscreen = shown.iter().rev().find(|id| here(id)).copied().filter(|id| {
        l.wm.get(*id).is_some_and(|w| {
            w.fullscreen()
                && (l.wm.focused() == Some(*id)
                    || (!w.sticky && w.desk != l.wm.current_desk() && l.wm.front_of(w.desk) == Some(*id)))
        })
    });
    if let Some(id) = top_fullscreen {
        window_items(st, id, origin, scale, (0.0, 0.0), 1.0, &mut out);
    }
    // With the shell's notification cards sliding in and out.
    crate::slides::top_layer(st, output, scale, &mut out);

    out.extend(crate::overlays::drag_feedback(st, output, scale));

    match desk_slide {
        Some(slide) => {
            // The Desk arriving on this display slides in; what else shows
            // (sticky windows, another display's reaching over) stays put,
            // and the Desk leaving slides out, unless it went to another
            // display.
            let l = st.lg.r();
            let t = slide.progress();
            let arriving = |id: &WinId| l.wm.get(*id).is_some_and(|w| !w.sticky && w.desk == slide.to);
            let list: Vec<(WinId, f64)> = shown
                .iter()
                .filter(|id| Some(**id) != top_fullscreen)
                .map(|id| (*id, if arriving(id) { (1.0 - t) * out_w * slide.dir } else { 0.0 }))
                .collect();
            let mut outgoing: Vec<(WinId, f64)> = if l.wm.is_visible(slide.from) {
                Vec::new()
            } else {
                l.wm.shown_stacking_on(slide.from)
                    .into_iter()
                    .filter(|id| l.wm.get(*id).is_some_and(|w| !w.sticky))
                    .map(|id| (id, -t * out_w * slide.dir))
                    .collect()
            };
            outgoing.extend(list);
            for (id, dx) in outgoing.into_iter().rev() {
                window_items(st, id, origin, scale, (dx, 0.0), 1.0, &mut out);
            }
        }
        None => {
            for id in shown.iter().rev() {
                if Some(*id) == top_fullscreen {
                    continue;
                }
                window_items(st, *id, origin, scale, (0.0, 0.0), 1.0, &mut out);
            }
        }
    }

    layer_items(output, Layer::Bottom, scale, &mut out);
    layer_items(output, Layer::Background, scale, &mut out);
    // Under GLES in Full, a blur behind the shell's frosted surfaces.
    crate::frost::behind(st, output, &mut out);
    out
}

/// A window's surfaces (popups are in the popup layer) and its
/// decorations, at its place plus `offset` points.
#[allow(clippy::too_many_arguments)]
pub fn window_items(
    st: &mut State,
    id: WinId,
    origin: (f64, f64),
    scale: f64,
    offset: (f64, f64),
    alpha: f32,
    out: &mut Scene,
) {
    let Some(w) = st.lg.r().wm.get(id) else { return };
    let rect = w.rect;
    let fullscreen = w.fullscreen();
    let at = (rect.x as f64 + offset.0, rect.y as f64 + offset.1);
    let Some(item) = window_item(&st.az, id, at, origin, scale, alpha) else { return };
    out.push(item);
    if fullscreen {
        return;
    }
    let Some(layout) = st.lg.r().decor_layout(&mut st.az.labels, id) else { return };
    let layout = shift_layout(&layout, offset);
    decoration_items(st, &layout, origin, scale, alpha, out);
}

/// A layout moved by an offset (for slides).
fn shift_layout(l: &Layout, offset: (f64, f64)) -> Layout {
    if offset == (0.0, 0.0) {
        return l.clone();
    }
    let (dx, dy) = (offset.0.round() as i32, offset.1.round() as i32);
    let mut l = l.clone();
    l.content = l.content.translate(dx, dy);
    l.frame = l.frame.translate(dx, dy);
    for t in &mut l.tabs {
        t.rect = t.rect.translate(dx, dy);
        t.close = t.close.translate(dx, dy);
        t.zoom = t.zoom.translate(dx, dy);
        t.title_x += dx as f64;
    }
    l
}

/// A layout's tabs, frame and shadow, with titles and glyphs.
pub fn decoration_items(st: &mut State, layout: &Layout, origin: (f64, f64), scale: f64, alpha: f32, out: &mut Scene) {
    let shadows = st.az.settings.appearance.effects.shadows();
    let mut palette = st.az.palette;
    if crate::VARIANT == Some("red") {
        palette.focus = aizome_theme::Rgba::hex(0xff0000);
    }
    let (labels, icons) = (&mut st.az.labels, &mut st.az.icons);
    let l = st.lg.m();
    // Tabs sliding into a group draw at their animated place.
    let mut layout = layout.clone();
    for t in &mut layout.tabs {
        let (dx, dy) = l.anims.tab_offset(t.win);
        if dx != 0.0 || dy != 0.0 {
            let (dx, dy) = (dx.round() as i32, dy.round() as i32);
            t.rect = t.rect.translate(dx, dy);
            t.close = t.close.translate(dx, dy);
            t.zoom = t.zoom.translate(dx, dy);
            t.title_x += dx as f64;
        }
    }
    let pieces = decor::pieces(&layout, &palette, shadows, scale);
    // Titles (and idle tabs' glyphs) above the tab fills.
    for t in &layout.tabs {
        let (_, ink, _) = decor::tab_colors(t.look, &palette);
        let title = l.wm.get(t.win).map(|w| w.title.clone()).unwrap_or_default();
        let face = if t.look == TabLook::Background { Face::TabLight } else { Face::Tab };
        let style = Style::new(face, typeface::TAB, ink);
        if t.look != TabLook::Focused {
            let app_id = l.wm.get(t.win).map(|w| w.app_id.clone()).unwrap_or_default();
            let glyph_x = t.close.x as f64 - 1.0;
            let glyph_y = t.rect.y as f64 + 1.0 + (TAB_INNER - decor::GLYPH) / 2.0;
            if let Some(e) =
                aizome_comp_core::icons::glyph_item(icons, &app_id, ink, (glyph_x, glyph_y), origin, scale, alpha)
            {
                out.push(e);
            }
        }
        let title = labels.fit(&title, face, typeface::TAB, t.title_max_w);
        if let Some(label) = labels.get(&title, style, scale, t.title_max_w + 1.0) {
            let x = t.title_x;
            let y = t.rect.y as f64 + 1.0 + ((TAB_INNER - label.h as f64) / 2.0).round();
            let loc: Point<i32, Physical> =
                Point::<f64, Logical>::from((x - origin.0, y - origin.1)).to_physical_precise_round(scale);
            out.push(SceneItem::Memory {
                buffer: label.buffer.clone(),
                loc: loc.to_f64(),
                alpha: Some(alpha),
                src: None,
                size: Some(Size::from((label.w, label.h))),
                kind: Kind::Unspecified,
            });
        }
    }
    let cache = &mut l.data(layout.win).decor;
    out.extend(cache.elements(&pieces, origin, scale, alpha).into_iter().map(SceneItem::Solid));
}

/// The height inside a tab under its top border.
const TAB_INNER: f64 = (decor::TAB_H - decor::BORDER) as f64;

impl Logic {
    /// A window's decoration layout (its group's, when stacked), or none
    /// for full-screen windows.
    pub fn decor_layout(&self, labels: &mut Labels, id: WinId) -> Option<Layout> {
        let w = self.wm.get(id)?;
        if w.fullscreen() {
            return None;
        }
        let (content, ssd, offset) = (w.rect, w.ssd, w.tab_offset);
        let members = self.wm.group_members(id);
        let active = members.iter().copied().find(|m| self.wm.is_group_active(*m)).unwrap_or(id);
        let focused = self.wm.focused().is_some_and(|f| members.contains(&f)) && !self.overlays.steals_focus_look();
        let titles: Vec<(WinId, String)> =
            members.iter().map(|m| (*m, self.wm.get(*m).map(|w| w.title.clone()).unwrap_or_default())).collect();
        let widths: Vec<(WinId, f64)> =
            titles.iter().map(|(m, t)| (*m, labels.measure(t, Face::Tab, typeface::TAB).0)).collect();
        Some(decor::layout(content, ssd, focused, &widths, active, offset))
    }
}
