//! Shadows and gradients.
//!
//! A shadow is the shape's coverage, blurred, moved by the offset, tinted
//! with the shadow's color and composited under the shape. The blur is
//! three box blurs in a row, which comes close to a Gaussian whose
//! standard deviation is half the blur radius, as `NSShadow` describes
//! its radius. The coverage is taken over all the pixels that can reach
//! the region being drawn (the region moved back by the offset, and the
//! blur's reach around it), not just the region: a shadow's pixels are the
//! same however the damage was cut, across scroll tiles too.
//!
//! Gradients become tiny-skia shaders, padded past their ends; one that
//! shouldn't extend there fills only the band it covers, which the main
//! thread makes the shape it fills (see `gradient`).

use std::cell::RefCell;

use tiny_skia::{Mask, PixmapMut, PixmapRef, Transform};

use crate::protocol::{GradientSpec, ShadowSpec};

/// A thread's shadow buffers, reused: the coverage, its alpha, a spare
/// for the blur, its columns' running sums, and what [`blur_coverage`]
/// works in (a band of rows on its own, some columns of that, which
/// columns are unlike the one before, the blurred coverage, a line of it).
#[derive(Default)]
struct Scratch {
    cover: Vec<u32>,
    alpha: Vec<u8>,
    tmp: Vec<u8>,
    sums: Vec<u32>,
    part: Vec<u8>,
    sub: Vec<u8>,
    unlike: Vec<bool>,
    out: Vec<u8>,
    row: [Vec<u8>; 2],
}

thread_local!(static SCRATCH: RefCell<Scratch> = RefCell::default());

/// How far (pixels) a shadow blurred by `blur` pixels reaches beyond its
/// shape.
pub fn reach(blur: f32) -> f32 {
    if blur <= 0.0 { 0.0 } else { blur * 1.5 + 2.0 }
}

/// The widths of three box blurs approximating a Gaussian of deviation
/// `sigma` (the usual construction, as in the CSS filter specification).
fn boxes(sigma: f32) -> [usize; 3] {
    let ideal = (12.0 * sigma * sigma / 3.0 + 1.0).sqrt();
    let mut lower = ideal.floor() as usize;
    if lower.is_multiple_of(2) {
        lower = lower.saturating_sub(1);
    }
    let upper = lower + 2;
    let m = ((12.0 * sigma * sigma - 3.0 * (lower * lower) as f32 - 12.0 * lower as f32 - 9.0)
        / (-4.0 * lower as f32 - 4.0))
        .round() as usize;
    [0, 1, 2].map(|i| if i < m { lower } else { upper })
}

/// Blur the row `src` into `dst` with a box of `size` pixels (odd, over
/// 1): each pixel the rounded mean of the `size` around it, zeros outside.
fn blur_row(src: &[u8], dst: &mut [u8], size: usize) {
    let (w, r) = (src.len(), size / 2);
    let mean = Mean::new(size as u32);
    let mut sum: u32 = src[..=r.min(w - 1)].iter().map(|&v| u32::from(v)).sum();
    for (i, out) in dst.iter_mut().enumerate() {
        *out = mean.of(sum);
        if i + r + 1 < w {
            sum += u32::from(src[i + r + 1]);
        }
        if i >= r {
            sum -= u32::from(src[i - r]);
        }
    }
}

/// Blur `a` (`w` × `h` coverage) in place with a box of `size` pixels (odd),
/// along rows, then along columns: each pixel the rounded mean of the
/// `size` around it, zeros outside. The columns go a row at a time with a
/// running sum per column, so both passes read memory in order.
fn box_blur(a: &mut [u8], tmp: &mut Vec<u8>, sums: &mut Vec<u32>, w: usize, h: usize, size: usize) {
    if size <= 1 || w == 0 || h == 0 {
        return;
    }
    let r = size / 2;
    let mean = Mean::new(size as u32);
    tmp.clear();
    tmp.resize(a.len(), 0);
    for (src, dst) in a.chunks_exact(w).zip(tmp.chunks_exact_mut(w)) {
        blur_row(src, dst, size);
    }
    sums.clear();
    sums.resize(w, 0);
    for row in tmp.chunks_exact(w).take(r + 1) {
        sums.iter_mut().zip(row).for_each(|(s, &v)| *s += u32::from(v));
    }
    for y in 0..h {
        a[y * w..(y + 1) * w].iter_mut().zip(sums.iter()).for_each(|(out, &s)| *out = mean.of(s));
        if y + r + 1 < h {
            sums.iter_mut().zip(&tmp[(y + r + 1) * w..(y + r + 2) * w]).for_each(|(s, &v)| *s += u32::from(v));
        }
        if y >= r {
            sums.iter_mut().zip(&tmp[(y - r) * w..(y - r + 1) * w]).for_each(|(s, &v)| *s -= u32::from(v));
        }
    }
}

/// The fewest rows or columns alike, beyond those the blur reaches from
/// either end, for a stretch of them to be blurred as one.
const ALIKE: usize = 4;

/// The stretches of `count` things in a row, each like the one before it
/// by `like`, whose neighbours as far as the blur reaches (`reach`) are all
/// in the same run of them: (from, to) of each, in order; none if they come
/// to under a quarter of everything, when going around them saves less
/// than it costs. Blurring across such a stretch changes nothing, a mean
/// of equal values being that value.
fn alike(count: usize, reach: usize, like: impl Fn(usize) -> bool) -> Vec<(usize, usize)> {
    let mut flat = Vec::new();
    let mut from = 0;
    while from < count {
        let to = (from + 1..count).find(|&i| !like(i)).unwrap_or(count);
        if to - from >= 2 * reach + ALIKE {
            flat.push((from + reach, to - reach));
        }
        from = to;
    }
    if flat.iter().map(|(a, b)| b - a).sum::<usize>() < count / 4 {
        flat.clear();
    }
    flat
}

/// The stretches of columns alike ([`alike`]) of `a`, rows `w` wide, with
/// `unlike` to work in. Coverage with no such columns (text, a disc) shows
/// it in its first few rows, and the rest aren't read.
fn columns_alike(a: &[u8], unlike: &mut Vec<bool>, w: usize, reach: usize) -> Vec<(usize, usize)> {
    unlike.clear();
    unlike.resize(w, false);
    let mut look = 8;
    for (y, row) in a.chunks_exact(w).enumerate() {
        unlike[1..].iter_mut().zip(row.windows(2)).for_each(|(u, pair)| *u |= pair[0] != pair[1]);
        if y + 1 == look {
            look *= 4;
            if alike(w, reach, |x| !unlike[x]).is_empty() {
                return Vec::new();
            }
        }
    }
    alike(w, reach, |x| !unlike[x])
}

/// Blur `line` along itself with each of `sizes`, with `spare` to work in.
fn blur_line(line: &mut Vec<u8>, spare: &mut Vec<u8>, sizes: [usize; 3]) {
    spare.clear();
    spare.resize(line.len(), 0);
    for size in sizes.into_iter().filter(|size| *size > 1) {
        blur_row(line, spare, size);
        std::mem::swap(line, spare);
    }
}

/// Blur the scratch's alpha (`w` × `h` coverage) with a box of each of
/// `sizes` in turn, as [`box_blur`] three times over does, leaving out
/// the work whose answer is known.
///
/// Where a row and every row the blur reaches from it are alike, blurring
/// along columns changes nothing, so each of them comes out as that row
/// blurred along itself, worked out once; and so for columns alike, blurred
/// along themselves. A box with a soft shadow is nearly all such rows and
/// columns (those between its corners, and the empty ones around it),
/// which leaves its corners to blur both ways: each with the rows and
/// columns that reach it, as on its own the three boxes read no further.
/// The pixels are the ones blurring everything gives, for a small part of
/// the work when what casts the shadow is large; coverage with nothing
/// alike is blurred where it is, as before.
fn blur_coverage(s: &mut Scratch, w: usize, h: usize, sizes: [usize; 3]) {
    let reach: usize = sizes.iter().map(|size| size / 2).sum();
    if w == 0 || h == 0 || reach == 0 {
        return;
    }
    let rows = alike(h, reach, |y| s.alpha[y * w..(y + 1) * w] == s.alpha[(y - 1) * w..y * w]);
    if rows.is_empty() {
        // One band, all of it.
        std::mem::swap(&mut s.alpha, &mut s.part);
        let columns = columns_alike(&s.part, &mut s.unlike, w, reach);
        if columns.is_empty() {
            for size in sizes {
                box_blur(&mut s.part, &mut s.tmp, &mut s.sums, w, h, size);
            }
            std::mem::swap(&mut s.alpha, &mut s.part);
        } else {
            s.out.clear();
            s.out.resize(w * h, 0);
            blur_around(s, w, h, sizes, columns, (0, h), 0);
            std::mem::swap(&mut s.alpha, &mut s.out);
        }
        return;
    }
    s.out.clear();
    s.out.resize(w * h, 0);
    let mut from = 0;
    for (f0, f1) in rows.into_iter().chain([(h, h)]) {
        if from < f0 {
            // The rows between two stretches, with the rows above and below
            // that reach them (zeros past the coverage's own edges).
            let (top, bottom) = (from.saturating_sub(reach), (f0 + reach).min(h));
            s.part.clear();
            s.part.extend_from_slice(&s.alpha[top * w..bottom * w]);
            let columns = columns_alike(&s.part, &mut s.unlike, w, reach);
            blur_around(s, w, bottom - top, sizes, columns, (from - top, f0 - top), from);
        }
        if f0 < f1 {
            let [row, spare] = &mut s.row;
            row.clear();
            row.extend_from_slice(&s.alpha[f0 * w..(f0 + 1) * w]);
            blur_line(row, spare, sizes);
            s.out[f0 * w..f1 * w].chunks_exact_mut(w).for_each(|out| out.copy_from_slice(row));
        }
        from = f1;
    }
    std::mem::swap(&mut s.alpha, &mut s.out);
}

/// Blur the scratch's part (`w` × `h`: a band of rows of the coverage, with
/// the rows that reach it) with each of `sizes`, and put its rows `keep`
/// into the scratch's out from row `at` on: the stretches of `columns`
/// alike as those columns blurred along themselves, the rest both ways.
fn blur_around(
    s: &mut Scratch,
    w: usize,
    h: usize,
    sizes: [usize; 3],
    columns: Vec<(usize, usize)>,
    keep: (usize, usize),
    at: usize,
) {
    let reach: usize = sizes.iter().map(|size| size / 2).sum();
    let out = &mut s.out[at * w..(at + keep.1 - keep.0) * w];
    let mut from = 0;
    for (f0, f1) in columns.into_iter().chain([(w, w)]) {
        if from < f0 {
            // The columns between two stretches, with those on either side
            // that reach them.
            let (left, right) = (from.saturating_sub(reach), (f0 + reach).min(w));
            let width = right - left;
            s.sub.clear();
            s.part.chunks_exact(w).for_each(|row| s.sub.extend_from_slice(&row[left..right]));
            for size in sizes {
                box_blur(&mut s.sub, &mut s.tmp, &mut s.sums, width, h, size);
            }
            for (out, sub) in out.chunks_exact_mut(w).zip(s.sub.chunks_exact(width).skip(keep.0)) {
                out[from..f0].copy_from_slice(&sub[from - left..f0 - left]);
            }
        }
        if f0 < f1 {
            let [column, spare] = &mut s.row;
            column.clear();
            column.extend(s.part.chunks_exact(w).map(|row| row[f0]));
            blur_line(column, spare, sizes);
            for (out, &v) in out.chunks_exact_mut(w).zip(&column[keep.0..keep.1]) {
                out[f0..f1].fill(v);
            }
        }
        from = f1;
    }
}

/// The rounded mean of `n` coverages from their sum, `(sum + n / 2) / n`,
/// as a multiply and a shift: exact for every sum `n` bytes can make, as
/// the error `m · n − 2⁴⁰` (at most `n`) times a sum (under `256 · n`) stays
/// under 2⁴⁰ for any box narrower than 65,536 pixels.
#[derive(Clone, Copy)]
struct Mean {
    half: u32,
    m: u64,
}

impl Mean {
    fn new(n: u32) -> Mean {
        Mean { half: n / 2, m: (1u64 << 40) / u64::from(n) + 1 }
    }

    #[inline]
    fn of(self, sum: u32) -> u8 {
        ((u64::from(sum + self.half) * self.m) >> 40) as u8
    }
}

/// Draw `spec`'s shadow of a shape under what `pm` (a region of the
/// canvas) holds, inside `mask`. `coverage` draws the shape (in any
/// opaque color) onto a transparent pixmap, with the region's transform
/// followed by the one it's given, which places the pixels that can cast
/// shadow into the region.
pub fn shadow(
    pm: &mut PixmapMut,
    spec: &ShadowSpec,
    scale: f32,
    mask: Option<&Mask>,
    coverage: impl FnOnce(&mut PixmapMut, Transform),
) {
    cast(pm, spec, scale, mask, |s, (ww, wh), (ox, oy)| {
        s.cover.clear();
        s.cover.resize(ww * wh, 0);
        let Some(mut target) = PixmapMut::from_bytes(super::as_bytes(&mut s.cover), ww as u32, wh as u32) else {
            return false;
        };
        coverage(&mut target, Transform::from_translate(ox as f32, oy as f32));
        s.alpha.clear();
        s.alpha.extend(s.cover.iter().map(|p| p.to_ne_bytes()[3]));
        true
    });
}

/// As [`shadow`], cast by premultiplied pixels (a group's) whose top left
/// is at `at` in the region: their alpha is the coverage, taken as it is
/// rather than drawn again.
pub fn shadow_of(
    pm: &mut PixmapMut,
    spec: &ShadowSpec,
    scale: f32,
    mask: Option<&Mask>,
    src: PixmapRef,
    at: (i32, i32),
) {
    cast(pm, spec, scale, mask, |s, (ww, wh), (ox, oy)| {
        s.alpha.clear();
        s.alpha.resize(ww * wh, 0);
        let (sw, sh) = (src.width() as isize, src.height() as isize);
        // Source pixel (x, y) is the window's (x + dx, y + dy).
        let (dx, dy) = (at.0 as isize + ox, at.1 as isize + oy);
        let (x0, x1) = (0.max(-dx), sw.min(ww as isize - dx));
        if x0 >= x1 {
            return true;
        }
        let data = src.data();
        for y in 0.max(-dy)..sh.min(wh as isize - dy) {
            let from = &data[((y * sw + x0) * 4) as usize..((y * sw + x1) * 4) as usize];
            let to = (y + dy) as usize * ww + (x0 + dx) as usize;
            for (a, p) in s.alpha[to..to + (x1 - x0) as usize].iter_mut().zip(from.as_chunks::<4>().0) {
                *a = p[3];
            }
        }
        true
    });
}

/// The shadow, from `coverage`, which fills the scratch's alpha (a window
/// the size it is given) with what casts it, offset as it is given.
fn cast(
    pm: &mut PixmapMut,
    spec: &ShadowSpec,
    scale: f32,
    mask: Option<&Mask>,
    coverage: impl FnOnce(&mut Scratch, (usize, usize), (isize, isize)) -> bool,
) {
    let (w, h) = (pm.width() as usize, pm.height() as usize);
    let blur = spec.blur * scale;
    // Moved by the offset (whole pixels; shadows are soft anyway).
    let (dx, dy) = ((spec.dx * scale).round() as isize, (spec.dy * scale).round() as isize);
    // The coverage that reaches the region: region pixel (x, y) shows the
    // blurred coverage at (x − dx, y − dy), which the blur takes from `r`
    // pixels around. The window puts that at (x + r, y + r).
    let r = reach(blur).ceil() as usize;
    let (ww, wh) = (w + 2 * r, h + 2 * r);
    SCRATCH.with(|scratch| {
        let mut fresh = Scratch::default();
        let mut held = scratch.try_borrow_mut();
        let s = match held.as_deref_mut() {
            Ok(s) => s,
            Err(_) => &mut fresh,
        };
        if !coverage(s, (ww, wh), (r as isize + dx, r as isize + dy)) {
            return;
        }
        // Nothing reaches the region (the shape is empty, or all of it is
        // further off than its shadow goes): no shadow to blur or put down.
        if s.alpha.iter().all(|&a| a == 0) {
            return;
        }
        if blur > 0.0 {
            blur_coverage(s, ww, wh, boxes(blur / 2.0));
        }
        let c = spec.color.map(|v| v.clamp(0.0, 1.0));
        // The shadow's pixel at each coverage, worked out once.
        let shades: [u32; 256] = std::array::from_fn(|cov| {
            let a = cov as f32 / 255.0 * c[3];
            let ch = |v: f32| (v * a * 255.0 + 0.5) as u8;
            u32::from_ne_bytes([ch(c[0]), ch(c[1]), ch(c[2]), (a * 255.0 + 0.5) as u8])
        });
        let mask = mask.map(Mask::data);
        for (y, row) in pm.data_mut().as_chunks_mut::<4>().0.chunks_exact_mut(w).enumerate() {
            let covs = &s.alpha[(y + r) * ww + r..(y + r) * ww + r + w];
            let Some(m) = mask else {
                put_down(row, covs, &shades);
                continue;
            };
            for (x, (px, &cov)) in row.iter_mut().zip(covs).enumerate() {
                let cov = (u32::from(cov) * u32::from(m[y * w + x]) + 127) / 255;
                if cov != 0 {
                    *px = super::over(u32::from_ne_bytes(*px), shades[cov as usize]).to_ne_bytes();
                }
            }
        }
    });
}

/// Put a row of shadow down: `shades[cov]` over each pixel of `row`, for
/// its coverage in `covs`. A stretch of one coverage goes as one: none of
/// it is skipped, and all of it (inside what casts the shadow) is one shade
/// over each pixel, in a loop with no choice in it, which the compiler
/// spreads over a vector's lanes.
fn put_down(row: &mut [[u8; 4]], covs: &[u8], shades: &[u32; 256]) {
    // How many of `covs` are like its first.
    let run = |covs: &[u8]| {
        let mut n = 1;
        while n + 32 <= covs.len() && covs[n..n + 32] == [covs[0]; 32] {
            n += 32;
        }
        n + covs[n..].iter().take_while(|&&c| c == covs[0]).count()
    };
    let mut x = 0;
    while x < covs.len() {
        let end = x + run(&covs[x..]);
        if covs[x] != 0 {
            let shade = shades[usize::from(covs[x])];
            for px in &mut row[x..end] {
                *px = super::over(u32::from_ne_bytes(*px), shade).to_ne_bytes();
            }
        }
        x = end;
    }
}

/// The shader for a gradient in user space.
pub fn gradient(g: &GradientSpec) -> Option<tiny_skia::Shader<'static>> {
    let stops: Vec<tiny_skia::GradientStop> =
        g.stops.iter().map(|&(at, c)| tiny_skia::GradientStop::new(at.clamp(0.0, 1.0), super::ops::color(c))).collect();
    let (start, end) = (tiny_skia::Point::from_xy(g.start.0, g.start.1), tiny_skia::Point::from_xy(g.end.0, g.end.1));
    match g.radii {
        None => tiny_skia::LinearGradient::new(start, end, stops, tiny_skia::SpreadMode::Pad, Transform::identity()),
        Some((r0, r1)) => tiny_skia::RadialGradient::new(
            start,
            r0.max(0.0),
            end,
            r1.max(0.0),
            stops,
            tiny_skia::SpreadMode::Pad,
            Transform::identity(),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn three_boxes_blur_like_a_gaussian() {
        // A lit bar spreads symmetrically, fading with distance.
        let (w, h) = (41, 41);
        let (mut tmp, mut sums) = (Vec::new(), Vec::new());
        let mut spread = vec![0u8; w * h];
        spread[20 * w + 18..20 * w + 23].fill(255);
        for size in boxes(2.0) {
            box_blur(&mut spread, &mut tmp, &mut sums, w, h, size);
        }
        assert_eq!(spread[20 * w + 10], spread[20 * w + 30], "symmetric");
        assert!(spread[20 * w + 20] > spread[20 * w + 24] && spread[20 * w + 24] > spread[20 * w + 27]);
        assert_eq!(spread[20 * w + 38], 0, "and bounded");
    }

    /// The blur as defined: each pixel the rounded mean of the `size` around
    /// it along a row, then along a column, zeros outside.
    fn plain_blur(a: &mut [u8], w: usize, h: usize, size: usize) {
        let r = size as isize / 2;
        let mean = |sum: u32| ((sum + size as u32 / 2) / size as u32) as u8;
        let at = |a: &[u8], x: isize, y: isize| {
            if x < 0 || y < 0 || x >= w as isize || y >= h as isize {
                0
            } else {
                u32::from(a[y as usize * w + x as usize])
            }
        };
        let rows: Vec<u8> =
            (0..w * h).map(|i| mean((-r..=r).map(|d| at(a, (i % w) as isize + d, (i / w) as isize)).sum())).collect();
        for (i, out) in a.iter_mut().enumerate() {
            *out = mean((-r..=r).map(|d| at(&rows, (i % w) as isize, (i / w) as isize + d)).sum());
        }
    }

    #[test]
    fn box_blurs_are_exact_means() {
        let mut seed = 0x2545_f491_4f6c_dd1du64;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        let (mut tmp, mut sums) = (Vec::new(), Vec::new());
        for (w, h) in [(1, 1), (3, 70), (61, 5), (97, 43), (128, 128)] {
            for size in [1, 3, 5, 9, 33, 129, 201] {
                // Coverage as shadows see it: runs of solid, edges, nothing.
                let mut a: Vec<u8> =
                    (0..w * h).map(|_| if next() % 3 == 0 { 0 } else { (next() % 256) as u8 }).collect();
                a.iter_mut().take(w * h / 3).for_each(|v| *v = 255);
                let mut want = a.clone();
                plain_blur(&mut want, w, h, size);
                box_blur(&mut a, &mut tmp, &mut sums, w, h, size);
                assert!(a == want, "{w}x{h}, box of {size}");
            }
        }
        // The means are exact for every sum, at every width a box can be.
        for n in [1u32, 3, 7, 63, 255, 1023, 4095, 65_535] {
            let m = Mean::new(n);
            for sum in (0..=255 * n).step_by((n as usize / 64).max(1)).chain([255 * n]) {
                assert_eq!(u32::from(m.of(sum)), (sum + n / 2) / n, "sum {sum} of {n}");
            }
        }
    }

    #[test]
    fn rows_alike_blur_to_the_pixels_blurring_everything_gives() {
        let mut seed = 0x9e37_79b9_7f4a_7c15u64;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        let plain = |a: &[u8], w: usize, h: usize, sizes: [usize; 3]| {
            let (mut a, mut tmp, mut sums) = (a.to_vec(), Vec::new(), Vec::new());
            sizes.iter().for_each(|&size| box_blur(&mut a, &mut tmp, &mut sums, w, h, size));
            a
        };
        // Boxes with soft corners and edges as a rasterizer leaves them,
        // some reaching past the coverage's edges, alone and one over
        // another; a box of noise among them; nothing; and noise alone,
        // which has no rows alike.
        let mut cases: Vec<(usize, usize, Vec<u8>)> = Vec::new();
        for (w, h) in [(97, 240), (64, 64), (33, 500), (200, 131), (5, 90)] {
            for kind in 0..6 {
                let mut a = vec![0u8; w * h];
                let mut boxed = |a: &mut [u8], (x0, y0, x1, y1): (usize, usize, usize, usize), corner: usize, noise| {
                    for y in y0..y1.min(h) {
                        for x in x0..x1.min(w) {
                            let (dx, dy) = (x.min(x1 - 1 - x), y.min(y1 - 1 - y));
                            let edge = (y - y0).min(y1 - 1 - y).min(x - x0).min(x1 - 1 - x);
                            let round = dx < corner && dy < corner && (corner - dx) + (corner - dy) > corner + 1;
                            a[y * w + x] = match (noise, round, edge) {
                                (true, ..) => (next() % 256) as u8,
                                (_, true, _) => 0,
                                (.., 0) => 141,
                                _ => 255,
                            };
                        }
                    }
                };
                match kind {
                    0 => boxed(&mut a, (w / 8, h / 10, w - w / 8, h - h / 10), 4, false),
                    1 => boxed(&mut a, (0, 0, w, h + 40), 6, false),
                    2 => {
                        boxed(&mut a, (2, h / 3, w - 1, h + 9), 3, false);
                        boxed(&mut a, (w / 3, h / 2, w / 2 + 1, h / 2 + 30), 2, false);
                    }
                    3 => {
                        boxed(&mut a, (1, 3, w - 1, h - 3), 5, false);
                        boxed(&mut a, (w / 4, h / 2, w / 2 + 1, h / 2 + 7), 0, true);
                    }
                    4 => {}
                    _ => a.iter_mut().for_each(|v| *v = (next() % 256) as u8),
                }
                cases.push((w, h, a));
            }
        }
        for (w, h, a) in cases {
            for blur in [1.0f32, 2.0, 4.0, 8.0, 13.5, 30.0] {
                let sizes = boxes(blur / 2.0);
                let mut s = Scratch { alpha: a.clone(), ..Scratch::default() };
                blur_coverage(&mut s, w, h, sizes);
                assert!(s.alpha == plain(&a, w, h, sizes), "{w}x{h}, blurred by {blur} (boxes {sizes:?})");
            }
        }
    }
}
