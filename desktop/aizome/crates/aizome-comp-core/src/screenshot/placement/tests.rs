//! Deterministic property sweeps: exercise combinations of scales, crop
//! phases, native sizes, negative positions and shared edges. These compare
//! pixels/geometry invariants, rather than reproducing the planner's branches.

use super::*;

const SCALES: &[f64] = &[0.5, 0.75, 1.0, 1.1, 1.25, 4.0 / 3.0, 1.5, 1.75, 2.0, 3.2, 4.0];

fn rect(x: i32, y: i32, w: i32, h: i32) -> LayoutRect {
    layout_rect(Rect::new(x, y, w, h)).unwrap()
}

fn output(bounds: LayoutRect, scale: f64, size: (u32, u32), color: Option<[u8; 4]>) -> CapturedOutput {
    let mut rgba = Vec::new();
    for y in 0..size.1 {
        for x in 0..size.0 {
            rgba.extend_from_slice(&color.unwrap_or([x as u8, y as u8, (x ^ y) as u8, 255]));
        }
    }
    CapturedOutput::new(bounds, scale, Capture { width: size.0, height: size.1, rgba }).unwrap()
}

fn native_size(bounds: LayoutRect, scale: f64) -> (u32, u32) {
    ((bounds.size.w as f64 * scale).round() as u32, (bounds.size.h as f64 * scale).round() as u32)
}

/// Inspect every planned access before executing, against the borrowed
/// images. Also returns the number of destination pixels the job will write.
fn assert_in_bounds(job: &Blit<'_, '_>) -> usize {
    let src = Rectangle::from_size(job.source.size());
    let dst = Rectangle::from_size(job.destination.size());
    match &job.placement {
        Placement::Empty => 0,
        Placement::ExactCopy(copy) => {
            assert!(src.contains_rect(copy.source));
            assert!(
                dst.contains_rect(Rectangle::new(copy.destination, (copy.source.size.w, copy.source.size.h).into(),))
            );
            copy.source.size.w as usize * copy.source.size.h as usize
        }
        Placement::Resample(sample) => {
            assert!(dst.contains_rect(sample.destination));
            assert_eq!(sample.columns.len(), sample.destination.size.w as usize);
            assert_eq!(sample.rows.len(), sample.destination.size.h as usize);
            assert!(sample.columns.iter().all(|x| *x < src.size.w));
            assert!(sample.rows.iter().all(|y| *y < src.size.h));
            sample.destination.size.w as usize * sample.destination.size.h as usize
        }
    }
}

#[test]
fn exact_copies_and_fractional_crops_preserve_native_pixels_in_order() {
    for &scale in SCALES {
        let bounds = rect(-23, 17, 64, 48);
        let source = output(bounds, scale, native_size(bounds, scale), None);
        for x in 0..12 {
            for y in 0..9 {
                // Inside the output, and up to its far edges (where an area
                // dragged past them is clipped): a crop from mid-pixel must
                // not run one pixel past the capture.
                for (w, h) in [(13, 11), (64 - x, 48 - y)] {
                    let mut shot = Screenshot::new(rect(-23 + x, 17 + y, w, h), bounds.loc, scale).unwrap();
                    let job = shot.plan(&source).unwrap();
                    assert!(matches!(job.placement, Placement::ExactCopy(_)), "scale {scale}, crop ({x}, {y}) {w}×{h}");
                    assert_in_bounds(&job);
                    job.execute();
                    // Reference: crop the actual native image directly. No
                    // conversion of individual pixel centres back into points.
                    let sx = (x as f64 * scale).round() as usize;
                    let sy = (y as f64 * scale).round() as usize;
                    let size = shot.image.pixels().size();
                    for row in 0..size.h {
                        assert_eq!(
                            shot.image.pixels().row(row),
                            &source.image.row(sy as u32 + row)[sx..sx + size.w as usize],
                            "scale {scale}, crop ({x}, {y}) {w}×{h}, row {row}",
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn thin_crops_below_one_scale_keep_native_pixels_even_at_far_edges() {
    let bounds = rect(-23, 17, 64, 48);
    for scale in [0.5, 0.75] {
        let source = output(bounds, scale, native_size(bounds, scale), None);
        let native = source.image.size();
        let edge = |at: i32| (at as f64 * scale).round() as u32;
        for x in 0..64 {
            for y in 0..48 {
                for (w, h) in [(1, 1), (1, 48 - y), (64 - x, 1)] {
                    let mut shot = Screenshot::new(rect(-23 + x, 17 + y, w, h), bounds.loc, scale).unwrap();
                    let job = shot.plan(&source).unwrap();
                    assert_in_bounds(&job);
                    job.execute();
                    let size = shot.image.pixels().size();
                    assert_eq!((size.w, size.h), ((edge(x + w) - edge(x)).max(1), (edge(y + h) - edge(y)).max(1)));
                    let sx = edge(x).min(native.w - 1) as usize;
                    let sy = edge(y).min(native.h - 1);
                    for row in 0..size.h {
                        assert_eq!(
                            shot.image.pixels().row(row),
                            &source.image.row(sy + row)[sx..sx + size.w as usize],
                            "scale {scale}, crop ({x}, {y}) {w}×{h}, row {row}",
                        );
                    }
                }
            }
        }
    }
}

/// 150% beside 100%, cropped from a point mid-pixel at 150%: the larger
/// output's pixels run whole to where its capture ends, and its neighbour
/// starts right there.
#[test]
fn a_crop_into_a_smaller_scale_neighbour_keeps_whole_native_pixels() {
    let blue = [0, 0, 255, 255];
    let left = rect(0, 0, 64, 48);
    let right = rect(64, 0, 40, 30);
    let a = output(left, 1.5, native_size(left, 1.5), None);
    let b = output(right, 1.0, native_size(right, 1.0), Some(blue));
    let mut shot = Screenshot::new(rect(1, 1, 90, 40), left.loc, 1.5).unwrap();
    let job = shot.plan(&a).unwrap();
    assert!(matches!(job.placement, Placement::ExactCopy(_)));
    job.execute();
    shot.plan(&b).unwrap().execute();
    // From native (2, 2): round(1.5). The right output ends 30 points down,
    // round(30 × 1.5) − 2 = 43 pixels.
    let edge = 96 - 2;
    let size = shot.image.pixels().size();
    assert_eq!((size.w, size.h), (137 - 2, 62 - 2));
    for row in 0..size.h {
        let pixels = shot.image.pixels().row(row);
        assert_eq!(&pixels[..edge], &a.image.row(row + 2)[2..], "row {row}");
        let beside = if row < 43 { blue } else { [0; 4] };
        assert!(pixels[edge..].iter().all(|p| *p == beside), "row {row}");
    }
}

#[test]
fn positive_fractional_offsets_keep_the_first_native_pixel() {
    for (scale, offset, first) in [(1.25, 2, 3), (1.5, 1, 2), (1.75, 2, 4)] {
        let bounds = rect(offset, offset, 32, 24);
        let source = output(bounds, scale, native_size(bounds, scale), None);
        let mut shot = Screenshot::new(rect(0, 0, 20, 20), bounds.loc, scale).unwrap();
        let job = shot.plan(&source).unwrap();
        assert!(matches!(job.placement, Placement::ExactCopy(_)));
        job.execute();
        assert_eq!(shot.image.pixels().row(first)[first as usize], source.image.row(0)[0]);
    }
}

#[test]
fn every_planned_access_is_in_bounds_and_only_its_region_is_written() {
    let mut copies = 0;
    let mut resamples = 0;
    let mut empty = 0;
    for &own in SCALES {
        for &scale in SCALES.iter().filter(|&&s| s >= own) {
            for x in [-20, -1, 0, 1, 13, 40] {
                for y in [-15, -1, 0, 1, 9, 30] {
                    for (w, h) in [(1, 1), (7, 5), (19, 13)] {
                        let bounds = rect(x, y, w, h);
                        let (pw, ph) = native_size(bounds, own);
                        // Logical bounds need not round-trip to the actual
                        // mode's pixel size; exercise either rounding edge.
                        for adjust in [-1, 0, 1] {
                            let size = ((pw as i32 + adjust).max(1) as u32, (ph as i32 + adjust).max(1) as u32);
                            let source = output(bounds, own, size, None);
                            // The grid is the crop's corner, the source's, or
                            // another output's.
                            for grid in [Point::from((0, 0)), bounds.loc, Point::from((3, -2))] {
                                let mut shot = Screenshot::new(rect(0, 0, 17, 11), grid, scale).unwrap();
                                let job = shot.plan(&source).unwrap();
                                let count = assert_in_bounds(&job);
                                match job.placement {
                                    Placement::ExactCopy(_) => copies += 1,
                                    Placement::Resample(_) => resamples += 1,
                                    Placement::Empty => empty += 1,
                                }
                                job.execute();
                                assert_eq!(
                                    shot.image.rgba().as_chunks::<4>().0.iter().filter(|p| p[3] == 255).count(),
                                    count
                                );
                                assert!(
                                    shot.image.rgba().as_chunks::<4>().0.iter().all(|p| p[3] == 255 || *p == [0; 4])
                                );
                            }
                        }
                    }
                }
            }
        }
    }
    assert!(copies > 0 && resamples > 0 && empty > 0);
}

/// The grid on the source's corner, or on another output's (`grid` from
/// the source's).
fn translated_shot(own: f64, scale: f64, grid: (i64, i64), dx: i32, dy: i32) -> Image {
    let bounds = rect(1 + dx, 3 + dy, 23, 19);
    let source = output(bounds, own, native_size(bounds, own), None);
    let grid = bounds.loc + Point::from(grid);
    let mut shot = Screenshot::new(rect(5 + dx, 7 + dy, 25, 21), grid, scale).unwrap();
    shot.plan(&source).unwrap().execute();
    shot.into_image()
}

#[test]
fn moving_output_and_crop_together_does_not_change_pixels() {
    for &own in SCALES {
        for &scale in SCALES.iter().filter(|&&s| s >= own) {
            for grid in [(0, 0), (-5, 2)] {
                let original = translated_shot(own, scale, grid, 0, 0);
                for (dx, dy) in [(-101, 77), (63, -109), (i32::MIN + 100, i32::MAX - 100)] {
                    let shifted = translated_shot(own, scale, grid, dx, dy);
                    assert_eq!(
                        original.rgba(),
                        shifted.rgba(),
                        "scale {own} -> {scale}, grid {grid:?}, shift ({dx}, {dy})"
                    );
                }
            }
        }
    }
}

#[test]
fn adjacent_outputs_share_one_rounded_edge_without_gaps_or_overlaps() {
    let red = [255, 0, 0, 255];
    let blue = [0, 0, 255, 255];
    for &own in SCALES {
        for &other in SCALES {
            let scale = own.max(other);
            for left in [1, 3, 7, 11] {
                for right in [1, 4, 9] {
                    for crop_at in 0..left {
                        for horizontal in [true, false] {
                            let layout = |at, length| {
                                if horizontal { rect(at, -3, length, 5) } else { rect(-3, at, 5, length) }
                            };
                            let a = layout(0, left);
                            let b = layout(left, right);
                            // On the grid of the output at the larger scale.
                            let (grid, from) = if own >= other { (a.loc, 0) } else { (b.loc, left) };
                            let a = output(a, own, native_size(a, own), Some(red));
                            let b = output(b, other, native_size(b, other), Some(blue));
                            let crop = layout(crop_at, left + right - crop_at);
                            let mut ab = Screenshot::new(crop, grid, scale).unwrap();
                            let mut ba = Screenshot::new(crop, grid, scale).unwrap();
                            ab.plan(&a).unwrap().execute();
                            ab.plan(&b).unwrap().execute();
                            ba.plan(&b).unwrap().execute();
                            ba.plan(&a).unwrap().execute();
                            // Reversing the writes exposes overlap; checking
                            // every expected pixel exposes holes and seams.
                            assert_eq!(ab.image.rgba(), ba.image.rgba());
                            let on_grid = |at: i32| ((at - from) as f64 * scale).round() as i64;
                            let edge = (on_grid(left) - on_grid(crop_at)) as u32;
                            let size = ab.image.pixels().size();
                            for y in 0..size.h {
                                for x in 0..size.w {
                                    let before = if horizontal { x < edge } else { y < edge };
                                    assert_eq!(ab.image.pixels().row(y)[x as usize], if before { red } else { blue });
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn smaller_scales_sample_the_source_pixel_under_each_centre() {
    let bounds = rect(0, 0, 8, 4);
    let source = output(bounds, 1.25, (10, 5), None);
    let mut shot = Screenshot::new(bounds, bounds.loc, 2.0).unwrap();
    let job = shot.plan(&source).unwrap();
    assert!(matches!(job.placement, Placement::Resample(_)));
    job.execute();
    let columns: Vec<_> = shot.image.pixels().row(0).iter().map(|p| p[0]).collect();
    assert_eq!(columns, [0, 0, 1, 2, 2, 3, 4, 4, 5, 5, 6, 7, 7, 8, 9, 9]);

    // The low band from #318, on the grid of the 2× panel at (0, 0):
    // source rows 600..678, each repeated twice.
    let source = output(rect(1440, 0, 1, 1080), 1.0, (1, 1080), None);
    let mut shot = Screenshot::new(rect(1440, 600, 1, 78), Point::from((0, 0)), 2.0).unwrap();
    shot.plan(&source).unwrap().execute();
    for y in 0..156 {
        assert_eq!(shot.image.pixels().row(y)[0][1], (600 + y / 2) as u8);
    }
}

#[test]
fn invalid_scales_and_unrepresentable_images_are_rejected() {
    for scale in [0.0, -1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY, f64::MAX] {
        assert!(Screenshot::new(rect(0, 0, 2, 2), Point::from((0, 0)), scale).is_err());
    }
    assert!(Screenshot::new(rect(0, 0, i32::MAX, i32::MAX), Point::from((0, 0)), 4.0).is_err());
    assert!(layout_rect(Rect::new(0, 0, -1, 2)).is_err());
}
