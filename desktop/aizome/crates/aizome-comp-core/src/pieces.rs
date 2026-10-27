//! Flat rectangles of colour with stable identities: what decorations,
//! the logic's overlays and the core's recovery overlay are drawn from.
//! [`DecorCache`] turns pieces into solid-colour render elements whose
//! identities persist from frame to frame, so the damage tracker redraws
//! only what moved or changed colour.

use aizome_theme::Rgba;
use smithay::backend::renderer::element::solid::SolidColorRenderElement;
use smithay::backend::renderer::element::{Id, Kind};
use smithay::backend::renderer::utils::CommitCounter;
use smithay::utils::{Physical, Rectangle};

use crate::Rect;

/// A flat rectangle of colour, in points (fractions allowed for strokes
/// that round to whole pixels).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Piece {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub color: Rgba,
}

impl Piece {
    pub fn new(x: f64, y: f64, w: f64, h: f64, color: Rgba) -> Piece {
        Piece { x, y, w, h, color }
    }
    pub fn rect(r: Rect, color: Rgba) -> Piece {
        Piece::new(r.x as f64, r.y as f64, r.w as f64, r.h as f64, color)
    }
}

/// Snaps a piece in points to whole pixels at a scale, relative to an
/// output's origin: edges round independently, so neighbours meet.
pub fn to_physical(x: f64, y: f64, w: f64, h: f64, origin: (f64, f64), scale: f64) -> Rectangle<i32, Physical> {
    let x0 = ((x - origin.0) * scale).round() as i32;
    let y0 = ((y - origin.1) * scale).round() as i32;
    let x1 = ((x + w - origin.0) * scale).round() as i32;
    let y1 = ((y + h - origin.1) * scale).round() as i32;
    Rectangle::new((x0, y0).into(), ((x1 - x0).max(0), (y1 - y0).max(0)).into())
}

/// Stable identities for a window's decoration pieces, so unchanged pieces
/// cause no damage.
#[derive(Debug, Default)]
pub struct DecorCache {
    slots: Vec<(Id, CommitCounter, Rgba)>,
}

impl DecorCache {
    /// One piece's element in slot `index` (overlays number their pieces
    /// in drawing order, so unchanged pieces keep their identity).
    pub fn element(
        &mut self,
        index: usize,
        piece: &Piece,
        origin: (f64, f64),
        scale: f64,
        alpha: f32,
    ) -> Option<SolidColorRenderElement> {
        while self.slots.len() <= index {
            self.slots.push((Id::new(), CommitCounter::default(), Rgba::hex(0)));
        }
        let slot = &mut self.slots[index];
        if slot.2 != piece.color {
            slot.2 = piece.color;
            slot.1.increment();
        }
        let geo = to_physical(piece.x, piece.y, piece.w, piece.h, origin, scale);
        if geo.size.w <= 0 || geo.size.h <= 0 {
            return None;
        }
        let c = piece.color.to_premultiplied_f32();
        let color = [c[0] * alpha, c[1] * alpha, c[2] * alpha, c[3] * alpha];
        Some(SolidColorRenderElement::new(slot.0.clone(), geo, slot.1, color, Kind::Unspecified))
    }

    /// Render elements for pieces (given back to front), front first as
    /// smithay wants them.
    pub fn elements(
        &mut self,
        pieces: &[Piece],
        origin: (f64, f64),
        scale: f64,
        alpha: f32,
    ) -> Vec<SolidColorRenderElement> {
        while self.slots.len() < pieces.len() {
            self.slots.push((Id::new(), CommitCounter::default(), Rgba::hex(0)));
        }
        let mut out = Vec::with_capacity(pieces.len());
        for (i, piece) in pieces.iter().enumerate() {
            let slot = &mut self.slots[i];
            if slot.2 != piece.color {
                slot.2 = piece.color;
                slot.1.increment();
            }
            let geo = to_physical(piece.x, piece.y, piece.w, piece.h, origin, scale);
            if geo.size.w <= 0 || geo.size.h <= 0 {
                continue;
            }
            let c = piece.color.to_premultiplied_f32();
            let color = [c[0] * alpha, c[1] * alpha, c[2] * alpha, c[3] * alpha];
            out.push(SolidColorRenderElement::new(slot.0.clone(), geo, slot.1, color, Kind::Unspecified));
        }
        out.reverse();
        out
    }
}

/// A dashed outline: dashes `dash` long with `gap` between, `t` thick.
pub fn dashed(out: &mut Vec<Piece>, r: Rect, t: f64, dash: f64, gap: f64, color: Rgba) {
    let (x, y, w, h) = (r.x as f64, r.y as f64, r.w as f64, r.h as f64);
    let along = |len: f64, mut put: Box<dyn FnMut(f64, f64) + '_>| {
        let mut s = 0.0;
        while s < len {
            let l = dash.min(len - s);
            put(s, l);
            s += dash + gap;
        }
    };
    let mut horiz = Vec::new();
    along(w, Box::new(|s, l| horiz.push((s, l))));
    for (s, l) in &horiz {
        out.push(Piece::new(x + s, y, *l, t, color));
        out.push(Piece::new(x + s, y + h - t, *l, t, color));
    }
    let mut vert = Vec::new();
    along(h - 2.0 * t, Box::new(|s, l| vert.push((s, l))));
    for (s, l) in &vert {
        out.push(Piece::new(x, y + t + s, t, *l, color));
        out.push(Piece::new(x + w - t, y + t + s, t, *l, color));
    }
}
