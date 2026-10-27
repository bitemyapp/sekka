//! Numbers with their digit groups apart. `calc` parts the groups with thin
//! spaces (U+2009); the gap between them is laid out here, a fifth of an
//! em of the text's size (a thin space's width in typesetting), rather
//! than left to the font: IBM Plex Mono is monospaced and gives its thin
//! space a whole cell (0.6 em) like any other character, and a font
//! without one hands it to whichever face has it.

use aizome_kit::sidestep_ui::{Canvas, TextLayout};
use aizome_kit::text::TextSpec;
use aizome_kit::widget::PaintCx;
use aizome_kit::{Align, Point, Rect, Size};

/// What parts the groups.
pub const SEPARATOR: char = '\u{2009}';

/// The gap between groups, in ems of the text's size.
pub const GAP_EM: f64 = 0.2;

/// A line laid out with its groups apart.
pub struct Grouped {
    /// Each group and where it starts.
    parts: Vec<(f64, TextLayout)>,
    size: Size,
}

impl Grouped {
    pub fn new(cx: &mut PaintCx, text: &str, spec: &TextSpec) -> Grouped {
        let gap = spec.font.size * GAP_EM;
        let mut parts = Vec::new();
        let (mut x, mut height) = (0.0_f64, 0.0_f64);
        for (i, group) in text.split(SEPARATOR).enumerate() {
            if i > 0 {
                x += gap;
            }
            if group.is_empty() {
                continue;
            }
            let l = cx.layout_text(group, spec, None);
            let s = l.size();
            height = height.max(s.height);
            parts.push((x, l));
            x += s.width;
        }
        Grouped { parts, size: Size::new(x, height) }
    }

    pub fn size(&self) -> Size {
        self.size
    }

    /// Draw it with its top left at `at`.
    pub fn draw(&self, canvas: &mut Canvas, at: Point) {
        for (x, l) in &self.parts {
            canvas.draw_text(l, Point::new(at.x + x, at.y));
        }
    }
}

/// Which end of a number too long for its room gives way to an ellipsis.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Cut {
    /// The start: the low digits matter most.
    Start,
    /// The end, as text that doesn't fit ends.
    End,
}

/// `text`, cut at `cut` with an ellipsis so that, with its groups apart,
/// it fits `width` (or down to the ellipsis alone).
pub fn fit(cx: &mut PaintCx, text: &str, spec: &TextSpec, width: f64, cut: Cut) -> String {
    let w = Grouped::new(cx, text, spec).size().width;
    if w <= width {
        return text.to_owned();
    }
    let chars: Vec<char> = text.chars().collect();
    // Monospaced: characters are about the same width, so start near it.
    let per = w / chars.len().max(1) as f64;
    let mut keep = ((width / per).floor() as usize).saturating_sub(1).min(chars.len());
    loop {
        let s: String = match cut {
            Cut::Start => std::iter::once('…').chain(chars[chars.len() - keep..].iter().copied()).collect(),
            Cut::End => chars[..keep].iter().copied().chain(std::iter::once('…')).collect(),
        };
        if keep == 0 || Grouped::new(cx, &s, spec).size().width <= width {
            return s;
        }
        keep -= 1;
    }
}

/// Draw one line of `text` in `rect` as [`PaintCx::draw_text_in`] does
/// (centred down it, placed across it by `align`, ending in an ellipsis
/// if it doesn't fit), with its digit groups apart: the width it took.
pub fn draw_in(cx: &mut PaintCx, text: &str, spec: &TextSpec, rect: Rect, align: Align) -> f64 {
    if !text.contains(SEPARATOR) {
        return cx.draw_text_in(text, spec, rect, align);
    }
    if rect.width() <= 0.0 {
        return 0.0;
    }
    let fitted = fit(cx, text, spec, rect.width(), Cut::End);
    let g = Grouped::new(cx, &fitted, spec);
    let s = g.size();
    let x = rect.x0 + align.offset(s.width.min(rect.width()), rect.width());
    let y = rect.y0 + ((rect.height() - s.height) / 2.0).round();
    g.draw(cx.canvas, Point::new(x.round(), y));
    s.width.min(rect.width())
}
