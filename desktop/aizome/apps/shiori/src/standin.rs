//! Stand-ins: what shows of a tile while its drawing at this size comes,
//! made from the drawings there are of its page at other sizes (the last
//! zoom's tiles, the preview) scaled to this one. They're made here, on
//! several threads, of the part of the tile in view, so the window copies
//! them one to one as it does the drawings: scaled as the window drew, a
//! zoom's first frame took hundreds of milliseconds at 4K. A zoom shows at
//! once, and sharpens as the drawings come.

use std::cell::RefCell;
use std::ops::Range;
use std::sync::Arc;

use aizome_kit::prelude::Image;

use crate::protocol::TileKey;

/// Rows of a stand-in made together on a thread.
const BAND: u32 = 64;

/// The fewest pixels worth more threads than the caller's.
const SPREAD: usize = 1 << 18;

/// The most bytes of stand-ins' pixels kept to be used again.
const SPARE_BYTES: usize = 64 << 20;

thread_local! {
    /// Stand-ins' pixels, oldest first, kept for the next ones of the same
    /// size once their images have gone: a zoom then neither allocates
    /// them nor has the system fault in and clear fresh pages.
    static SPARES: RefCell<Vec<Arc<[u8]>>> = const { RefCell::new(Vec::new()) };
}

/// A drawing to stand in from: where it is, and its pixels.
type Source<'a> = (TileKey, &'a [[u8; 4]]);

/// Stand-ins for `wanted` (parts of tiles: their page's size, and where),
/// each from the drawings of its page given with it, of other sizes and
/// any part of the page. What none of them has is paper. Those with
/// nothing to stand in from are left out.
pub fn make(wanted: Vec<(TileKey, Vec<(TileKey, Image)>)>) -> Vec<(TileKey, Image)> {
    // Each one's drawings in sizes, the furthest from its own first.
    let jobs: Vec<(TileKey, Vec<Vec<Source>>)> = wanted
        .iter()
        .filter_map(|(k, from)| {
            let mut from: Vec<Source> = from
                .iter()
                .filter(|(s, _)| s.size != k.size && s.w > 0 && s.h > 0 && covers(s, k))
                .filter_map(|(s, img)| Some((*s, img.premultiplied_pixels().filter(|p| p.len() == s.bytes())?)))
                .map(|(s, px)| (s, px.as_chunks::<4>().0))
                .collect();
            // Those smaller than the tile from the smallest up, then larger
            // ones from the largest down to the nearest.
            from.sort_by_key(|(s, _)| if s.size.0 >= k.size.0 { (1, u32::MAX - s.size.0) } else { (0, s.size.0) });
            let mut sizes: Vec<Vec<Source>> = Vec::new();
            for f in from {
                match sizes.last_mut() {
                    Some(same) if same[0].0.size == f.0.size => same.push(f),
                    _ => sizes.push(vec![f]),
                }
            }
            (!sizes.is_empty()).then_some((*k, sizes))
        })
        .collect();
    let pixels: usize = jobs.iter().map(|(k, _)| k.w as usize * k.h as usize).sum();
    let threads = if pixels < SPREAD {
        1
    } else {
        std::thread::available_parallelism().map_or(1, |n| n.get()).saturating_sub(1).clamp(1, 16)
    };
    // Each one's pixels, made where they'll stay (no copy into the image
    // after): spare ones of its size, or new ones, paper to start, made on
    // the threads.
    let mut spare: Vec<Option<Arc<[u8]>>> = SPARES.with_borrow_mut(|spares| {
        jobs.iter()
            .map(|(k, _)| {
                let i = spares.iter().position(|a| a.len() == k.bytes() && Arc::strong_count(a) == 1)?;
                Some(spares.remove(i))
            })
            .collect()
    });
    let paper = |k: &TileKey| std::iter::repeat_n(255u8, k.bytes()).collect::<Arc<[u8]>>();
    let new: Vec<&TileKey> = jobs.iter().zip(&spare).filter(|(_, s)| s.is_none()).map(|((k, _), _)| k).collect();
    let mut new: std::vec::IntoIter<Arc<[u8]>> = if threads <= 1 || new.len() <= 1 {
        new.iter().map(|k| paper(k)).collect::<Vec<_>>().into_iter()
    } else {
        std::thread::scope(|scope| {
            let made: Vec<_> = new
                .chunks(new.len().div_ceil(threads))
                .map(|part| scope.spawn(move || part.iter().map(|k| paper(k)).collect::<Vec<_>>()))
                .collect();
            made.into_iter().flat_map(|t| t.join().unwrap_or_else(|e| std::panic::resume_unwind(e))).collect::<Vec<_>>()
        })
        .into_iter()
    };
    let mut out: Vec<Arc<[u8]>> = jobs
        .iter()
        .zip(spare.iter_mut())
        .map(|((k, _), s)| s.take().or_else(|| new.next()).unwrap_or_else(|| paper(k)))
        .collect();
    let mut bands: Vec<(usize, u32, &mut [u8])> = Vec::new();
    for (j, buf) in out.iter_mut().enumerate() {
        let row = jobs[j].0.w as usize * 4;
        let Some(buf) = Arc::get_mut(buf) else { continue };
        for (n, rows) in buf.chunks_mut(row * BAND as usize).enumerate() {
            bands.push((j, n as u32 * BAND, rows));
        }
    }
    let draw = |bands: &mut [(usize, u32, &mut [u8])]| {
        for (j, y, rows) in bands.iter_mut() {
            let (k, sizes) = &jobs[*j];
            let h = (rows.len() / (k.w as usize * 4)) as u32;
            let ys = *y..*y + h;
            // From the nearest size that has all of these rows, and any
            // nearer over it; or from all there are, over paper (a spare
            // has the last stand-in's pixels).
            let all = k.w as usize * h as usize;
            let first =
                sizes.iter().rposition(|same| same.iter().map(|(s, _)| covered(k, &ys, s)).sum::<usize>() >= all);
            if first.is_none() {
                rows.fill(255);
            }
            for same in &sizes[first.unwrap_or(0)..] {
                for (s, px) in same {
                    draw_from(k, ys.clone(), rows, s, px);
                }
            }
        }
    };
    if threads <= 1 {
        draw(&mut bands);
    } else {
        let per = bands.len().div_ceil(threads);
        std::thread::scope(|scope| {
            for part in bands.chunks_mut(per) {
                scope.spawn(move || draw(part));
            }
        });
    }
    // Their pixels kept to be used again once the images are let go.
    SPARES.with_borrow_mut(|spares| {
        spares.extend(out.iter().cloned());
        let mut bytes: usize = spares.iter().map(|a| a.len()).sum();
        while bytes > SPARE_BYTES && !spares.is_empty() {
            bytes -= spares.remove(0).len();
        }
    });
    jobs.iter().zip(out).filter_map(|((k, _), px)| Some((*k, Image::from_premultiplied_rgba(k.w, k.h, px)?))).collect()
}

/// Whether drawing `s` has any of `k`'s part of the page.
fn covers(s: &TileKey, k: &TileKey) -> bool {
    let part = |at: u32, len: u32, size: u32| (f64::from(at) / f64::from(size), f64::from(at + len) / f64::from(size));
    let (sx, kx) = (part(s.at.0, s.w, s.size.0), part(k.at.0, k.w, k.size.0));
    let (sy, ky) = (part(s.at.1, s.h, s.size.1), part(k.at.1, k.h, k.size.1));
    sx.0 < kx.1 && kx.0 < sx.1 && sy.0 < ky.1 && ky.0 < sy.1
}

/// How many of rows `ys` of `k` (from its top) drawing `s` has.
fn covered(k: &TileKey, ys: &Range<u32>, s: &TileKey) -> usize {
    let xs = span(k.at.0, k.w, k.size.0, s.at.0, s.w, s.size.0);
    let rows = span(k.at.1 + ys.start, ys.end - ys.start, k.size.1, s.at.1, s.h, s.size.1);
    xs.len() * rows.len()
}

/// The pixels of a tile `at` along and `len` long, at `size`, whose centres
/// fall on drawing `s`'s part (`s_at`, `s_len`) at `s_size`: the page's
/// pixels along that side, from the page's edge.
fn span(at: u32, len: u32, size: u32, s_at: u32, s_len: u32, s_size: u32) -> Range<u32> {
    let f = f64::from(s_size) / f64::from(size);
    let lo = (f64::from(s_at) / f - 0.5).ceil().max(f64::from(at)) as u32;
    let hi = (f64::from(s_at + s_len) / f - 0.5).ceil().min(f64::from(at + len)).max(0.0) as u32;
    lo..hi.max(lo)
}

/// Where each of `range` (page pixels at the tile's size) falls on the
/// drawing: its two nearest pixels (from the drawing's own edge) and how
/// far it is from the first to the second, in 256ths.
fn taps(range: Range<u32>, f: f64, s_at: u32, s_len: u32) -> Vec<(usize, usize, u32)> {
    let last = i64::from(s_len) - 1;
    range
        .map(|x| {
            let at = (f64::from(x) + 0.5) * f - 0.5 - f64::from(s_at);
            let first = at.floor();
            let i = first as i64;
            let t = ((at - first) * 256.0).round() as u32;
            (i.clamp(0, last) as usize, (i + 1).clamp(0, last) as usize, t.min(256))
        })
        .collect()
}

/// `a` to `b` by `t` 256ths, two channels at a time.
#[inline]
fn lerp(a: u32, b: u32, t: u32) -> u32 {
    let u = 256 - t;
    let rb = (((a & 0x00ff_00ff) * u + (b & 0x00ff_00ff) * t) >> 8) & 0x00ff_00ff;
    let ag = (((a >> 8) & 0x00ff_00ff) * u + ((b >> 8) & 0x00ff_00ff) * t) & 0xff00_ff00;
    rb | ag
}

/// Draw rows `ys` (of `k`, from its top) of what drawing `s` (pixels
/// `px`) has of them into `rows`: bilinear, or, from a drawing over twice
/// as large, the mean of four of its pixels spread over each, so text
/// doesn't sparkle as much.
fn draw_from(k: &TileKey, ys: Range<u32>, rows: &mut [u8], s: &TileKey, px: &[[u8; 4]]) {
    let (fx, fy) = (f64::from(s.size.0) / f64::from(k.size.0), f64::from(s.size.1) / f64::from(k.size.1));
    let xs = span(k.at.0, k.w, k.size.0, s.at.0, s.w, s.size.0);
    let page_ys = span(k.at.1 + ys.start, ys.end - ys.start, k.size.1, s.at.1, s.h, s.size.1);
    if xs.is_empty() || page_ys.is_empty() {
        return;
    }
    let stride = s.w as usize;
    let row = k.w as usize;
    let (x0, y0) = (k.at.0, k.at.1 + ys.start);
    let first = (xs.start - x0) as usize;
    let rows = rows.as_chunks_mut::<4>().0;
    if fx > 2.0 || fy > 2.0 {
        // The pixels a quarter and three quarters of the way across and
        // down each: four, however much larger, which is enough for what
        // shows a moment, at bilinear's cost.
        let at = |v: u32, j: u32, f: f64, s_at: u32, s_len: u32| {
            let p = ((f64::from(v) + (f64::from(j) + 0.5) / 2.0) * f).floor() as i64 - i64::from(s_at);
            p.clamp(0, i64::from(s_len) - 1) as usize
        };
        let cols: Vec<[usize; 2]> = xs.map(|x| [0, 1].map(|i| at(x, i, fx, s.at.0, s.w))).collect();
        // Their mean, two channels at a time: each sum of four fits its half.
        let mean = |sum: u32| ((sum + 0x0002_0002) >> 2) & 0x00ff_00ff;
        for y in page_ys {
            let src = [0, 1].map(|j| &px[at(y, j, fy, s.at.1, s.h) * stride..][..stride]);
            let out = &mut rows[(y - y0) as usize * row..][..row];
            for (o, cs) in out[first..].iter_mut().zip(&cols) {
                let (mut rb, mut ag) = (0, 0);
                for r in src {
                    for c in cs {
                        let p = u32::from_le_bytes(r[*c]);
                        rb += p & 0x00ff_00ff;
                        ag += (p >> 8) & 0x00ff_00ff;
                    }
                }
                *o = (mean(rb) | (mean(ag) << 8)).to_le_bytes();
            }
        }
        return;
    }
    let cols = taps(xs, fx, s.at.0, s.w);
    // A row of the drawing scaled across, kept for the rows of the tile
    // that fall between it and the next (several, zoomed in).
    let across = |r: usize, into: &mut Vec<u32>| {
        let src = &px[r * stride..][..stride];
        into.clear();
        into.extend(cols.iter().map(|&(c0, c1, t)| lerp(u32::from_le_bytes(src[c0]), u32::from_le_bytes(src[c1]), t)));
    };
    let mut top = (usize::MAX, Vec::with_capacity(cols.len()));
    let mut bottom = (usize::MAX, Vec::with_capacity(cols.len()));
    for (y, (r0, r1, ty)) in page_ys.clone().zip(taps(page_ys, fy, s.at.1, s.h)) {
        if top.0 != r0 {
            if bottom.0 == r0 {
                std::mem::swap(&mut top, &mut bottom);
            } else {
                across(r0, &mut top.1);
                top.0 = r0;
            }
        }
        if bottom.0 != r1 {
            across(r1, &mut bottom.1);
            bottom.0 = r1;
        }
        let out = &mut rows[(y - y0) as usize * row..][..row];
        for (o, (&a, &b)) in out[first..].iter_mut().zip(top.1.iter().zip(&bottom.1)) {
            *o = lerp(a, b, ty).to_le_bytes();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(size: (u32, u32), at: (u32, u32), w: u32, h: u32) -> TileKey {
        TileKey { page: 0, size, at, w, h }
    }

    /// A drawing whose pixels say where they are: red across, green down.
    fn ramp(k: &TileKey) -> Image {
        let mut px = Vec::new();
        for y in 0..k.h {
            for x in 0..k.w {
                px.extend([(k.at.0 + x) as u8, (k.at.1 + y) as u8, 7, 255]);
            }
        }
        Image::from_premultiplied_rgba(k.w, k.h, px).unwrap()
    }

    fn at(img: &Image, x: u32, y: u32) -> [u8; 4] {
        let px = img.premultiplied_pixels().unwrap();
        let i = (y * img.pixel_size().0 + x) as usize * 4;
        [px[i], px[i + 1], px[i + 2], px[i + 3]]
    }

    #[test]
    fn a_tile_is_made_from_the_drawings_at_another_size() {
        // The page drawn 100 × 100 in four tiles, standing in for its top
        // left at 200 × 200: each pixel from where it falls, across the
        // drawings' edges.
        let small = [(0, 0), (50, 0), (0, 50), (50, 50)].map(|at| key((100, 100), at, 50, 50));
        let from: Vec<(TileKey, Image)> = small.iter().map(|k| (*k, ramp(k))).collect();
        let k = key((200, 200), (0, 0), 128, 128);
        let made = make(vec![(k, from)]);
        assert_eq!(made.len(), 1);
        let (got, img) = &made[0];
        assert_eq!((*got, img.pixel_size()), (k, (128, 128)));
        for (x, y) in [(0, 0), (40, 7), (99, 101), (100, 100), (127, 127)] {
            let p = at(img, x, y);
            let want = |v: u32| ((f64::from(v) + 0.5) / 2.0 - 0.5).clamp(0.0, 99.0);
            assert!((f64::from(p[0]) - want(x)).abs() <= 1.0, "{x},{y}: {p:?}");
            assert!((f64::from(p[1]) - want(y)).abs() <= 1.0, "{x},{y}: {p:?}");
            assert_eq!(p[2..], [7, 255]);
        }
    }

    #[test]
    fn the_nearest_size_shows_and_paper_where_nothing_was_drawn() {
        // A preview of the whole page, and the last zoom's top half: the
        // top half comes from the larger, the rest from the preview.
        let k = key((300, 300), (0, 0), 300, 300);
        let preview = key((30, 30), (0, 0), 30, 30);
        let half = key((400, 400), (0, 0), 400, 200);
        let solid = |k: &TileKey, v: u8| Image::from_premultiplied_rgba(k.w, k.h, vec![v; k.bytes()]).unwrap();
        let made = make(vec![(k, vec![(half, solid(&half, 200)), (preview, solid(&preview, 50))])]);
        let img = &made[0].1;
        assert_eq!(at(img, 150, 20), [200; 4]);
        assert_eq!(at(img, 150, 280), [50; 4]);
        // From the half alone, below it is paper.
        let made = make(vec![(k, vec![(half, solid(&half, 200))])]);
        assert_eq!(at(&made[0].1, 150, 280), [255; 4]);
        // Nothing to stand in from: none made.
        assert!(make(vec![(k, Vec::new())]).is_empty());
    }

    #[test]
    fn much_larger_drawings_are_averaged() {
        // Stripes two pixels wide, four times as large: grey, not stripes.
        let big = key((400, 400), (0, 0), 400, 400);
        let mut px = Vec::new();
        for _ in 0..400 {
            for x in 0..400 {
                let v = if x / 2 % 2 == 0 { 0 } else { 255 };
                px.extend([v, v, v, 255]);
            }
        }
        let k = key((100, 100), (0, 0), 100, 100);
        let made = make(vec![(k, vec![(big, Image::from_premultiplied_rgba(400, 400, px).unwrap())])]);
        for x in [0, 33, 99] {
            let p = at(&made[0].1, x, 50);
            assert!((i32::from(p[0]) - 128).abs() <= 1, "{x}: {p:?}");
        }
    }

    #[test]
    fn pixels_are_used_again_once_let_go_and_nothing_of_them_shows() {
        let k = key((200, 200), (0, 0), 64, 64);
        let solid = |k: &TileKey, v: u8| Image::from_premultiplied_rgba(k.w, k.h, vec![v; k.bytes()]).unwrap();
        let whole = key((100, 100), (0, 0), 100, 100);
        let first = make(vec![(k, vec![(whole, solid(&whole, 9))])]);
        let pixels = first[0].1.premultiplied_pixels().unwrap().as_ptr();
        drop(first);
        // The same size again, from a drawing of the page's top 32 rows
        // (at this size) alone: those, and paper below, not the 9s.
        let top = key((100, 100), (0, 0), 100, 16);
        let again = make(vec![(k, vec![(top, solid(&top, 50))])]);
        let img = &again[0].1;
        assert_eq!(img.premultiplied_pixels().unwrap().as_ptr(), pixels, "not used again");
        assert_eq!(at(img, 10, 5), [50; 4]);
        assert_eq!(at(img, 10, 50), [255; 4]);
    }

    #[test]
    fn many_tiles_are_made_on_several_threads_alike() {
        let small = key((512, 512), (0, 0), 512, 512);
        let from = vec![(small, ramp(&small))];
        let wanted: Vec<_> = (0..4).map(|i| (key((1024, 1024), (0, i * 256), 1024, 256), from.clone())).collect();
        let one = make(vec![wanted[2].clone()]);
        let all = make(wanted);
        assert_eq!(all.len(), 4);
        assert_eq!(all[2].1.premultiplied_pixels(), one[0].1.premultiplied_pixels());
    }
}
