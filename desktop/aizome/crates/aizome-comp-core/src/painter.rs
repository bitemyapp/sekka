//! Drawing overlays in Aizome's look (the logic's and the core's recovery
//! overlay): flat rectangles, 1pt bevels, hard
//! 3pt shadows, labels set by Sidestep, app icons and live thumbnails, in
//! painter's order. Pieces keep their identities between frames by their
//! place in the drawing order, so an unchanged overlay causes no damage.

use aizome_theme::{Palette, Rgba};
use smithay::backend::renderer::element::Kind;
use smithay::utils::{Logical, Physical, Point, Rectangle};

use crate::icons::Icons;
use crate::pieces::{DecorCache, Piece};
use crate::scene::{Scene, SceneItem};
use crate::state::Aizome;
use crate::text::{Face, Labels, Style};
use crate::{Rect, WinId};

enum Item {
    Piece(Piece),
    /// Front to back.
    Items(Vec<SceneItem>),
}

pub struct Painter {
    items: Vec<Item>,
    pub origin: (f64, f64),
    pub scale: f64,
    /// Everything fades by this (overlays coming in).
    pub alpha: f32,
    /// The opaque colour under everything drawn, when nothing else is
    /// (the overview covers the desktop: the frame's clear colour). Set,
    /// see-through pieces are drawn as the colours they make (see
    /// [`Painter::finish`]): worth it under pixman, where a blend reads
    /// every pixel it covers. On the GPU blending costs nothing, and opaque
    /// pieces only give the damage tracker more to cut around (0.3 ms more a
    /// frame of the overview fading in under GLES).
    pub ground: Option<Rgba>,
}

/// A rectangle in points with fractions.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Box2 {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Box2 {
    pub fn new(x: f64, y: f64, w: f64, h: f64) -> Box2 {
        Box2 { x, y, w, h }
    }
    pub fn contains(&self, x: f64, y: f64) -> bool {
        x >= self.x && y >= self.y && x < self.x + self.w && y < self.y + self.h
    }
    pub fn right(&self) -> f64 {
        self.x + self.w
    }
    pub fn bottom(&self) -> f64 {
        self.y + self.h
    }
    pub fn inset(&self, d: f64) -> Box2 {
        Box2::new(self.x + d, self.y + d, (self.w - 2.0 * d).max(0.0), (self.h - 2.0 * d).max(0.0))
    }
    pub fn rect(&self) -> Rect {
        Rect::new(self.x.round() as i32, self.y.round() as i32, self.w.round() as i32, self.h.round() as i32)
    }
}

impl Painter {
    pub fn new(origin: (f64, f64), scale: f64, alpha: f32) -> Painter {
        Painter { items: Vec::new(), origin, scale, alpha, ground: None }
    }

    pub fn fill(&mut self, b: Box2, color: Rgba) {
        if b.w > 0.0 && b.h > 0.0 && color.a > 0 {
            self.items.push(Item::Piece(Piece::new(b.x, b.y, b.w, b.h, color)));
        }
    }

    /// A 1pt outline.
    pub fn outline(&mut self, b: Box2, t: f64, color: Rgba) {
        self.fill(Box2::new(b.x, b.y, b.w, t), color);
        self.fill(Box2::new(b.x, b.bottom() - t, b.w, t), color);
        self.fill(Box2::new(b.x, b.y + t, t, b.h - 2.0 * t), color);
        self.fill(Box2::new(b.right() - t, b.y + t, t, b.h - 2.0 * t), color);
    }

    /// A dashed outline.
    pub fn dashed(&mut self, b: Box2, t: f64, color: Rgba) {
        let mut v = Vec::new();
        crate::pieces::dashed(&mut v, b.rect(), t, 6.0, 4.0, color);
        for p in v {
            self.items.push(Item::Piece(p));
        }
    }

    /// A panel in Aizome's look: the hard 3pt shadow, a 1pt `bevel_lo`
    /// edge, the fill and a 1pt `bevel_hi` top highlight.
    pub fn panel(&mut self, b: Box2, fill: Rgba, p: &Palette, shadow: bool) {
        if shadow {
            self.fill(Box2::new(b.right(), b.y + 3.0, 3.0, b.h), p.slab);
            self.fill(Box2::new(b.x + 3.0, b.bottom(), b.w - 3.0, 3.0), p.slab);
        }
        self.fill(b, p.bevel_lo);
        self.fill(b.inset(1.0), fill);
        self.fill(Box2::new(b.x + 1.0, b.y + 1.0, b.w - 2.0, 1.0), p.bevel_hi);
    }

    /// Text with its top left at (x, y), shortened to `max_w`; returns its
    /// width.
    #[allow(clippy::too_many_arguments)]
    pub fn text(&mut self, labels: &mut Labels, text: &str, style: Style, x: f64, y: f64, max_w: f64) -> f64 {
        let fitted = labels.fit(text, style.face, style.size(), max_w);
        let Some(label) = labels.get(&fitted, style, self.scale, max_w + 1.0) else { return 0.0 };
        let loc: Point<i32, Physical> =
            Point::<f64, Logical>::from((x - self.origin.0, y - self.origin.1)).to_physical_precise_round(self.scale);
        self.items.push(Item::Items(vec![SceneItem::Memory {
            buffer: label.buffer.clone(),
            loc: loc.to_f64(),
            alpha: Some(self.alpha),
            src: None,
            size: Some((label.w, label.h).into()),
            kind: Kind::Unspecified,
        }]));
        label.w as f64
    }

    /// Text centred vertically in a box of height `h` from `y`.
    #[allow(clippy::too_many_arguments)]
    pub fn text_mid(
        &mut self,
        labels: &mut Labels,
        text: &str,
        style: Style,
        x: f64,
        y: f64,
        h: f64,
        max_w: f64,
    ) -> f64 {
        let (_, th) = labels.measure(text, style.face, style.size());
        self.text(labels, text, style, x, y + ((h - th.ceil()) / 2.0).round(), max_w)
    }

    /// Text centred in a box.
    pub fn text_center(&mut self, labels: &mut Labels, text: &str, style: Style, b: Box2) -> f64 {
        let (tw, th) = labels.measure(text, style.face, style.size());
        let w = tw.min(b.w);
        self.text(labels, text, style, (b.x + (b.w - w) / 2.0).round(), (b.y + (b.h - th.ceil()) / 2.0).round(), b.w)
    }

    /// An app's icon, `size` points square, with its top left at (x, y).
    #[allow(clippy::too_many_arguments)]
    #[allow(clippy::too_many_arguments)]
    pub fn icon(
        &mut self,
        icons: &mut Icons,
        app_id: &str,
        tint: crate::icons::Tint,
        x: f64,
        y: f64,
        size: i32,
        alpha: f32,
    ) {
        let scale = self.scale;
        let origin = self.origin;
        if let Some(tex) = icons.texture(app_id, size, scale.ceil().max(1.0) as u32, tint) {
            let item = crate::icons::icon_item(tex, (x, y), origin, scale, alpha * self.alpha);
            self.items.push(Item::Items(vec![item]));
        }
    }

    /// A window's live content scaled into a box.
    pub fn thumbnail(&mut self, az: &Aizome, id: WinId, b: Box2, alpha: f32) {
        let e =
            crate::render::thumbnail_item(az, id, (b.x, b.y, b.w, b.h), self.origin, self.scale, alpha * self.alpha);
        if let Some(e) = e {
            self.items.push(Item::Items(vec![e]));
        }
    }

    /// The scene items, front first, with stable identities from `cache`.
    ///
    /// With a [`Painter::ground`], a see-through piece (its own alpha, or
    /// the painter's while fading) lying wholly on one flat colour is drawn as the opaque colour the
    /// two make: the same pixels, but filled rather than blended, and it
    /// hides what's under it from the renderer. Under pixman a blend
    /// costs a read of every pixel it covers; the overview fading in
    /// blended the whole output several times a frame.
    pub fn finish(self, cache: &mut DecorCache) -> Scene {
        let flat = self.flatten();
        let mut out = Vec::new();
        let mut slot = 0;
        for (item, flat) in self.items.into_iter().zip(flat) {
            match item {
                Item::Piece(p) => {
                    let e = match flat {
                        Some(c) => cache.element(slot, &Piece { color: c, ..p }, self.origin, self.scale, 1.0),
                        None => cache.element(slot, &p, self.origin, self.scale, self.alpha),
                    };
                    if let Some(e) = e {
                        out.push(vec![SceneItem::Solid(e)]);
                    }
                    slot += 1;
                }
                Item::Items(e) => out.push(e),
            }
        }
        out.into_iter().rev().flatten().collect()
    }

    /// For each item, the opaque colour a piece shows as, when it can be
    /// drawn as one (see [`Painter::finish`]).
    fn flatten(&self) -> Vec<Option<Rgba>> {
        if self.ground.is_none() {
            return vec![None; self.items.len()];
        }
        let rect = |p: &Piece| crate::pieces::to_physical(p.x, p.y, p.w, p.h, self.origin, self.scale);
        let bounds: Vec<Option<Rectangle<i32, Physical>>> = self
            .items
            .iter()
            .map(|i| match i {
                Item::Piece(p) => Some(rect(p)),
                Item::Items(items) => items_bounds(items, self.scale),
            })
            .collect();
        let mut flat: Vec<Option<Rgba>> = Vec::with_capacity(self.items.len());
        for (i, item) in self.items.iter().enumerate() {
            let Item::Piece(p) = item else {
                flat.push(None);
                continue;
            };
            let a = p.color.a as f32 / 255.0 * self.alpha;
            let r = bounds[i].unwrap_or_default();
            if a >= 1.0 {
                flat.push(Some(p.color));
                continue;
            }
            if r.is_empty() {
                flat.push(None);
                continue;
            }
            // What's under it: the nearest item below that touches it must
            // be a flat colour covering all of it.
            let mut under = self.ground;
            for j in (0..i).rev() {
                let Some(b) = bounds[j] else {
                    // Something of unknown extent: it may be under.
                    under = None;
                    break;
                };
                if !b.overlaps(r) {
                    continue;
                }
                under = if b.contains_rect(r) { flat[j] } else { None };
                break;
            }
            flat.push(under.map(|u| over(p.color, a, u)));
        }
        flat
    }
}

/// `c` at alpha `a` (its own alpha times any fading) over an opaque colour.
fn over(c: Rgba, a: f32, under: Rgba) -> Rgba {
    let mix = |s: u8, d: u8| (s as f32 * a + d as f32 * (1.0 - a)).round().clamp(0.0, 255.0) as u8;
    Rgba { r: mix(c.r, under.r), g: mix(c.g, under.g), b: mix(c.b, under.b), a: 255 }
}

/// Where scene items draw, in pixels, when that's known.
fn items_bounds(items: &[SceneItem], scale: f64) -> Option<Rectangle<i32, Physical>> {
    let mut all: Option<Rectangle<i32, Physical>> = None;
    for item in items {
        let r = match item {
            SceneItem::Memory { loc, size: Some(size), .. } => {
                let s = size.to_f64().to_physical(scale);
                Rectangle::new(loc.to_i32_floor(), (s.w.ceil() as i32 + 1, s.h.ceil() as i32 + 1).into())
            }
            SceneItem::Thumb { crop, .. } => *crop,
            SceneItem::Solid(e) => {
                use smithay::backend::renderer::element::Element;
                e.geometry(scale.into())
            }
            _ => return None,
        };
        all = Some(all.map_or(r, |a| a.merge(r)));
    }
    all
}

/// Text styles the overlays use.
pub fn semibold(size: f64, color: Rgba) -> Style {
    Style::new(Face::UiSemibold, size, color)
}
pub fn bold(size: f64, color: Rgba) -> Style {
    Style::new(Face::UiBold, size, color)
}
pub fn mono(size: f64, color: Rgba) -> Style {
    Style::new(Face::Mono, size, color)
}
pub fn mono_bold(size: f64, color: Rgba) -> Style {
    Style::new(Face::MonoBold, size, color)
}

#[cfg(test)]
mod tests {
    use smithay::backend::allocator::Fourcc;
    use smithay::backend::renderer::element::Element;
    use smithay::backend::renderer::element::memory::MemoryRenderBuffer;
    use smithay::utils::Transform;

    use super::*;

    /// Each solid's colour (straight, 0–255) and whether it's opaque, back
    /// to front.
    fn solids(scene: &Scene) -> Vec<([u8; 4], bool)> {
        scene
            .iter()
            .rev()
            .filter_map(|i| match i {
                SceneItem::Solid(e) => {
                    let c = e.color().components();
                    let a = c[3];
                    let ch = |v: f32| if a > 0.0 { (v / a * 255.0).round() as u8 } else { 0 };
                    Some(([ch(c[0]), ch(c[1]), ch(c[2]), (a * 255.0).round() as u8], e.alpha() >= 1.0))
                }
                _ => None,
            })
            .collect()
    }

    #[test]
    fn see_through_pieces_on_flat_colour_become_opaque() {
        let mut cache = DecorCache::default();
        let mut pt = Painter::new((0.0, 0.0), 2.0, 0.5);
        pt.ground = Some(Rgba::hex(0x000000));
        // White at half (the painter fading) over black; then a box at half
        // over that.
        pt.fill(Box2::new(0.0, 0.0, 100.0, 100.0), Rgba::hex(0xffffff));
        pt.fill(Box2::new(10.0, 10.0, 20.0, 20.0), Rgba::hex(0x0000ff));
        let s = solids(&pt.finish(&mut cache));
        assert_eq!(s, vec![([128, 128, 128, 255], true), ([64, 64, 192, 255], true)]);
    }

    #[test]
    fn pieces_over_other_things_still_blend() {
        let mut cache = DecorCache::default();
        let mut pt = Painter::new((0.0, 0.0), 1.0, 1.0);
        pt.ground = Some(Rgba::hex(0x000000));
        pt.fill(Box2::new(0.0, 0.0, 100.0, 100.0), Rgba::hex(0x202020));
        // A label, then a see-through piece over it, and one beside it.
        let buffer = MemoryRenderBuffer::new(Fourcc::Abgr8888, (10, 10), 1, Transform::Normal, None);
        pt.items.push(Item::Items(vec![SceneItem::Memory {
            buffer,
            loc: (5.0, 5.0).into(),
            alpha: None,
            src: None,
            size: Some((10, 10).into()),
            kind: Kind::Unspecified,
        }]));
        pt.fill(Box2::new(0.0, 0.0, 20.0, 20.0), Rgba::hex(0xffffff).with_alpha(0.5));
        pt.fill(Box2::new(50.0, 50.0, 20.0, 20.0), Rgba::hex(0xffffff).with_alpha(0.5));
        let s = solids(&pt.finish(&mut cache));
        assert_eq!(s[1], ([255, 255, 255, 128], false));
        assert_eq!(s[2], ([144, 144, 144, 255], true));
    }

    #[test]
    fn without_a_ground_the_lowest_piece_blends() {
        let mut cache = DecorCache::default();
        let mut pt = Painter::new((0.0, 0.0), 1.0, 0.5);
        pt.fill(Box2::new(0.0, 0.0, 100.0, 100.0), Rgba::hex(0xffffff));
        let s = solids(&pt.finish(&mut cache));
        assert_eq!(s, vec![([255, 255, 255, 128], false)]);
    }
}
