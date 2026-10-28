use hotaru_sprites::{Error, Metrics, rasterize, supports};
fn metrics(w: u32, h: u32, t: u32) -> Metrics {
    Metrics {
        cell_width: w,
        cell_height: h,
        box_thickness: t,
    }
}
#[test]
fn complete_explicit_ranges_and_exclusions() {
    assert_eq!((0..=0x10ffff).filter(|cp| supports(*cp)).count(), 153);
    for cp in 0x256d..=0x2573 {
        assert_eq!(rasterize(cp, metrics(11, 24, 1)), Err(Error::Unsupported));
    }
    for cp in [0, 0x24ff, 0x25a0, 0x2800, 0x10ffff, u32::MAX] {
        assert_eq!(rasterize(cp, metrics(11, 24, 1)), Err(Error::Unsupported));
    }
}
#[test]
fn allocation_is_bounded_before_reservation() {
    for m in [
        metrics(0, 1, 1),
        metrics(1, 0, 1),
        metrics(1, 1, 0),
        metrics(1, 2, 2),
        metrics(u32::MAX, u32::MAX, u32::MAX),
    ] {
        assert_eq!(rasterize(0x2588, m), Err(Error::InvalidMetrics));
    }
    assert_eq!(
        rasterize(0x2588, metrics(4096, 4096, 1)),
        Err(Error::SizeLimit)
    );
}
#[test]
fn odd_quadrants_overlap_and_shades_are_alpha_not_dither() {
    let tl = rasterize(0x2598, metrics(9, 17, 1)).unwrap();
    let br = rasterize(0x2597, metrics(9, 17, 1)).unwrap();
    assert_eq!(tl.alpha.iter().filter(|a| **a == 255).count(), 5 * 9);
    assert_eq!(br.alpha.iter().filter(|a| **a == 255).count(), 5 * 9);
    assert_eq!(
        tl.alpha
            .iter()
            .zip(&br.alpha)
            .filter(|(a, b)| **a == 255 && **b == 255)
            .count(),
        1
    );
    for (cp, alpha) in [(0x2591, 64), (0x2592, 128), (0x2593, 192)] {
        let r = rasterize(cp, metrics(11, 24, 1)).unwrap();
        assert_eq!(r.alpha.iter().filter(|a| **a == alpha).count(), 11 * 24);
        assert!(r.alpha.iter().all(|a| *a == 0 || *a == alpha));
    }
}
#[test]
fn tiny_heavy_dash_uses_upstream_light_fallback() {
    assert_eq!(
        rasterize(0x2505, metrics(5, 11, 1)),
        rasterize(0x2500, metrics(5, 11, 1))
    );
    assert_eq!(
        rasterize(0x2507, metrics(11, 5, 1)),
        rasterize(0x2502, metrics(11, 5, 1))
    );
}
#[test]
fn ink_bounds_reconstruct_every_nonzero_pixel() {
    for w in 1..=17 {
        for h in 1..=19 {
            for cp in 0x2500..=0x259f {
                if !supports(cp) {
                    continue;
                }
                let r = rasterize(cp, metrics(w, h, 1)).unwrap();
                if let Some(b) = r.ink {
                    assert!(b.x + b.width <= r.width && b.y + b.height <= r.height);
                    for (y, row) in r.alpha.chunks_exact(r.width as usize).enumerate() {
                        for (x, a) in row.iter().enumerate() {
                            if *a != 0 {
                                assert!(
                                    x >= b.x as usize
                                        && x < (b.x + b.width) as usize
                                        && y >= b.y as usize
                                        && y < (b.y + b.height) as usize
                                );
                            }
                        }
                    }
                } else {
                    assert!(r.alpha.iter().all(|a| *a == 0));
                }
            }
        }
    }
}
