//! Diagnostic only: absolute cold-raster allocation/draw/bounds/free cost.
//! This is not a Ghostty comparison or a native-frame performance gate.
use hotaru_sprites::{Metrics, rasterize};
use std::{hint::black_box, time::Instant};
fn main() {
    let cps = (0x256d..=0x2573)
        .chain(0xe0b0..=0xe0bf)
        .chain([0xe0d2, 0xe0d4])
        .collect::<Vec<_>>();
    print!("[ ");
    for (index, (w, h, t, iterations)) in [
        (11, 24, 2, 100),
        (14, 30, 2, 100),
        (22, 49, 3, 100),
        (8, 12, 8, 100),
        (1365, 1365, 1365, 1),
    ]
    .into_iter()
    .enumerate()
    {
        let mut ns = Vec::new();
        let m = Metrics {
            cell_width: w,
            cell_height: h,
            box_thickness: t,
        };
        for _ in 0..11 {
            let start = Instant::now();
            for _ in 0..iterations {
                for &cp in &cps {
                    black_box(rasterize(black_box(cp), black_box(m)).unwrap());
                }
            }
            ns.push(start.elapsed().as_nanos() as u64 / (iterations * cps.len() as u64));
        }
        let mut sorted = ns.clone();
        sorted.sort_unstable();
        if index > 0 {
            print!(",");
        }
        print!(
            "{{\"metrics\":[{w},{h},{t}],\"glyphs\":25,\"iterations\":{iterations},\"ns_per_glyph\":{ns:?},\"median_ns\":{}}}",
            sorted[5]
        );
    }
    println!("]");
}
