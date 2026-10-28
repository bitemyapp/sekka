//! Braille Patterns U+2800–U+28FF, adapted from Ghostty's MIT-licensed
//! src/font/sprite/draw/braille.zig at 6ba144158577d033ccb6ed4b574e4013957c9161.
//! See LICENSE.ghostty. Dot sizing intentionally ignores stroke thickness.
use crate::{Metrics, Raster};

pub(super) fn draw(canvas: &mut Raster, metrics: Metrics, cp: u32) {
    let (xs, ys, dot) = layout(metrics.cell_width, metrics.cell_height);
    debug_assert!((0x2800..=0x28ff).contains(&cp));
    // Unicode's lower two dots are bits 6/7; the first six are column-major.
    for (bit, col, row) in [
        (0, 0, 0),
        (1, 0, 1),
        (2, 0, 2),
        (6, 0, 3),
        (3, 1, 0),
        (4, 1, 1),
        (5, 1, 2),
        (7, 1, 3),
    ] {
        if cp & (1 << bit) != 0 {
            canvas.rect(xs[col], ys[row], xs[col] + dot, ys[row] + dot, 255);
        }
    }
}

fn layout(width: u32, height: u32) -> ([u32; 2], [u32; 4], u32) {
    let mut dot = (width / 4).min(height / 8);
    let mut x_spacing = width / 4;
    let mut y_spacing = height / 8;
    let mut x_margin = x_spacing / 2;
    let mut y_margin = y_spacing / 2;
    // The initial dot is at most each spacing; consequently both complete
    // grids fit and subtraction is nonnegative, including dimensions below 8.
    let mut x_left = width - 2 * x_margin - x_spacing - 2 * dot;
    let mut y_left = height - 2 * y_margin - 3 * y_spacing - 4 * dot;

    // Keep the reference's priority order: nonzero dot, nonzero margins,
    // spacing, larger margins, then larger dots. Tiny metrics can remain blank.
    if x_left >= 2 && y_left >= 4 && dot == 0 {
        dot += 1;
        x_left -= 2;
        y_left -= 4;
    }
    if x_left >= 2 && x_margin == 0 {
        x_margin = 1;
        x_left -= 2;
    }
    if y_left >= 2 && y_margin == 0 {
        y_margin = 1;
        y_left -= 2;
    }
    if x_left >= 1 {
        x_spacing += 1;
        x_left -= 1;
    }
    if y_left >= 3 {
        y_spacing += 1;
        y_left -= 3;
    }
    if x_left >= 2 {
        x_margin += 1;
        x_left -= 2;
    }
    if y_left >= 2 {
        y_margin += 1;
        y_left -= 2;
    }
    if x_left >= 2 && y_left >= 4 {
        dot += 1;
        x_left -= 2;
        y_left -= 4;
    }
    // Upstream asserts x_left<=1 || y_left<=1, which rejects 14x23 with
    // two spare pixels on both axes despite every dot fitting. A further
    // increment needs 2 horizontal and 4 vertical pixels. This is the actual
    // no-more-growth invariant; raster coordinates remain unchanged.
    debug_assert!(x_left < 2 || y_left < 4);
    debug_assert!(2 * x_margin + 2 * dot + x_spacing <= width);
    debug_assert!(2 * y_margin + 4 * dot + 3 * y_spacing <= height);
    let xs = [x_margin, x_margin + dot + x_spacing];
    let step = dot + y_spacing;
    let ys = [
        y_margin,
        y_margin + step,
        y_margin + 2 * step,
        y_margin + 3 * step,
    ];
    (xs, ys, dot)
}

#[cfg(test)]
mod tests {
    use super::layout;

    #[test]
    fn every_accepted_dimension_keeps_disjoint_dots_inside_cell() {
        // No allocation: validate even pairs whose padded mask exceeds the
        // public 4 MiB cap, proving the arithmetic before any raster clipping.
        for width in 1..=4096 {
            for height in 1..=4096 {
                let (xs, ys, dot) = layout(width, height);
                assert!(xs[0] + dot <= xs[1]);
                assert!(xs[1] + dot <= width);
                assert!(ys.windows(2).all(|rows| rows[0] + dot <= rows[1]));
                assert!(ys[3] + dot <= height);
            }
        }
    }

    #[test]
    fn valid_metrics_do_not_repeat_upstream_remaining_pixel_assertion() {
        assert_eq!(layout(14, 23), ([2, 8], [2, 7, 12, 17], 2));
    }
}
