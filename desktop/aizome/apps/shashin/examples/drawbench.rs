//! How long Sidestep's rasterizer takes to draw a picture into a view at
//! 2×, by the way it's drawn: a scratch measurement, not part of the app.
use aizome_kit::sidestep_ui::{
    Appearance, Image, ImageOptions,
    kurbo::{Rect, Size},
    offscreen,
};
use std::time::Instant;

fn main() {
    let (w, h) = (6000u32, 4000u32);
    let px: Vec<u8> = (0..w * h)
        .flat_map(|i| {
            let x = i % w;
            let y = i / w;
            [(x % 256) as u8, (y % 256) as u8, 128, 255]
        })
        .collect();
    let full = Image::from_rgba(w, h, &px).unwrap();
    let view = Size::new(1400.0, 900.0);
    let (vw, vh) = (2800u32, 1800u32);
    let small: Vec<u8> = vec![90; (vw * vh * 4) as usize];
    let fit = Image::from_rgba(vw, vh, &small).unwrap();
    let cases: Vec<(&str, Image, Option<Rect>, Rect, bool)> = vec![
        ("one-to-one blit, nearest", fit.clone(), None, Rect::new(0.0, 0.0, 1400.0, 900.0), false),
        ("one-to-one blit, nearest, copy blend", fit.clone(), None, Rect::new(0.0, 0.0, 1400.0, 900.0), false),
        ("one-to-one blit, smooth", fit.clone(), None, Rect::new(0.0, 0.0, 1400.0, 900.0), true),
        ("one-to-one, half-pixel offset, smooth", fit.clone(), None, Rect::new(0.25, 0.25, 1400.25, 900.25), true),
        (
            "1.5x up from crop, smooth",
            full.clone(),
            Some(Rect::new(0.0, 0.0, 1866.0, 1200.0)),
            Rect::new(0.0, 0.0, 1400.0, 900.0),
            true,
        ),
        (
            "1.5x up from crop, nearest",
            full.clone(),
            Some(Rect::new(0.0, 0.0, 1866.0, 1200.0)),
            Rect::new(0.0, 0.0, 1400.0, 900.0),
            false,
        ),
        (
            "3x up from crop, nearest",
            full.clone(),
            Some(Rect::new(0.0, 0.0, 933.0, 600.0)),
            Rect::new(0.0, 0.0, 1400.0, 900.0),
            false,
        ),
        ("0.9x down from fit-ish, smooth", fit.clone(), None, Rect::new(0.0, 0.0, 1260.0, 810.0), true),
        ("full 6000 down to view (halvings), smooth", full.clone(), None, Rect::new(0.0, 0.0, 1350.0, 900.0), true),
    ];
    for (name, solid) in [("nothing drawn", false), ("solid fill", true)] {
        let mut best = f64::MAX;
        for _ in 0..6 {
            let t = Instant::now();
            let _ = offscreen::render(view, 2.0, Appearance::DARK, |c| {
                if solid {
                    c.fill_rect(Rect::new(0.0, 0.0, 1400.0, 900.0), aizome_kit::Color::rgb8(20, 20, 30));
                }
            });
            best = best.min(t.elapsed().as_secs_f64() * 1000.0);
        }
        println!("{name:45} {best:7.2} ms");
    }
    for (name, img, src, dst, smooth) in cases {
        let mut times = Vec::new();
        for _ in 0..6 {
            let t = Instant::now();
            let _ = offscreen::render(view, 2.0, Appearance::DARK, |c| {
                if name.contains("copy") {
                    c.set_blend_mode(aizome_kit::sidestep_ui::BlendMode::Copy);
                }
                c.draw_image_with(&img, src, dst, ImageOptions { smooth, ..ImageOptions::default() });
            });
            times.push(t.elapsed().as_secs_f64() * 1000.0);
        }
        println!(
            "{name:45} first {:7.2} ms, then {:7.2} ms",
            times[0],
            times[1..].iter().cloned().fold(f64::MAX, f64::min)
        );
    }
}
