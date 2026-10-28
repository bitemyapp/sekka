//! Bounded small-image filter preparation, preserving f32 samples until GPU
//! destination composition. The sampling polynomials/order follow tiny-skia
//! 0.12 highp.rs (BSD-3-Clause; Copyright 2018 Google Inc., 2020 Yevhenii
//! Reizner). This is not a window or terminal rasterization path.
use super::Fallback;
use crate::{
    protocol::{Op, Quality},
    raster::{Canvas, images::Pixels},
};
use tiny_skia::{FillRule, Mask, PathBuilder, Transform};

pub(super) fn filtered(canvas: &Canvas, op: &Op, region: [u32; 4]) -> Result<Option<Vec<u8>>, Fallback> {
    let Op::Image { image, src, dst, alpha, quality, tint, draw, .. } = op else {
        return Ok(None);
    };
    let Pixels::Rgba(data) = &image.pixels else {
        return Err(Fallback::Unsupported("image storage"));
    };
    if data.len() != image.width as usize * image.height as usize * 4 || draw.xf.kx != 0. || draw.xf.ky != 0. {
        return Err(Fallback::Unsupported("image layout"));
    }
    let base = canvas.transform().pre_concat(draw.xf);
    let placed = Transform::from_row(
        (dst.x1 - dst.x0) / (src.x1 - src.x0),
        0.,
        0.,
        (dst.y1 - dst.y0) / (src.y1 - src.y0),
        dst.x0,
        dst.y0,
    )
    .pre_translate(-src.x0, -src.y0);
    let device = base.pre_concat(placed);
    if device.sx <= 0.5 || device.sy <= 0.5 {
        return Err(Fallback::Unsupported("image mipmap"));
    }
    let xf = base.post_translate(-(region[0] as f32), -(region[1] as f32));
    let inv = placed.post_concat(xf).invert().ok_or(Fallback::Geometry)?;
    if inv.kx != 0. || inv.ky != 0. || !inv.is_finite() {
        return Err(Fallback::Unsupported("image affine"));
    }
    if *quality == Quality::None || inv.is_translate() || inv.is_identity() {
        return Ok(None);
    }
    let width = region[2] - region[0];
    let height = region[3] - region[1];
    let count = (width as usize).checked_mul(height as usize).ok_or(Fallback::Limit)?;
    if count > super::MAX_SMALL_PIXELS {
        return Err(Fallback::Limit);
    }
    let rect = tiny_skia::Rect::from_ltrb(dst.x0, dst.y0, dst.x1, dst.y1).ok_or(Fallback::Geometry)?;
    let path = PathBuilder::from_rect(rect);
    let mut mask = Mask::new(width, height).ok_or(Fallback::Limit)?;
    mask.fill_path(&path, FillRule::Winding, false, xf);
    let tinted;
    let data = if let Some(t) = tint {
        tinted = data
            .chunks_exact(4)
            .flat_map(|p| {
                let a = p[3] as f32 / 255. * t[3].clamp(0., 1.);
                let ch = |v: f32| (v.clamp(0., 1.) * a * 255. + 0.5) as u8;
                [ch(t[0]), ch(t[1]), ch(t[2]), (a * 255. + 0.5) as u8]
            })
            .collect::<Vec<_>>();
        &tinted[..]
    } else {
        data
    };
    let mut out = Vec::with_capacity(count * 16);
    let near = |t: f32| t * (t * ((-21. / 18.) * t + 27. / 18.) + 9. / 18.) + 1. / 18.;
    let far = |t: f32| (t * t) * ((7. / 18.) * t - 6. / 18.);
    for y in 0..height {
        for x in 0..width {
            let mut rgba = [0f32; 4];
            if mask.data()[(y * width + x) as usize] != 0 {
                let cx = (x as f32 + 0.5) * inv.sx + inv.tx;
                let cy = (y as f32 + 0.5) * inv.sy + inv.ty;
                let fx = (cx + 0.5) - (cx + 0.5).floor();
                let fy = (cy + 0.5) - (cy + 0.5).floor();
                let (wx, wy, n, start) = if *quality == Quality::High {
                    (
                        [far(1. - fx), near(1. - fx), near(fx), far(fx)],
                        [far(1. - fy), near(1. - fy), near(fy), far(fy)],
                        4,
                        -1.5,
                    )
                } else {
                    ([1. - fx, fx, 0., 0.], [1. - fy, fy, 0., 0.], 2, -0.5)
                };
                let mut sy = cy + start;
                for wy in &wy[..n] {
                    let mut sx = cx + start;
                    for wx in &wx[..n] {
                        let xx = sx.max(0.).min(f32::from_bits((image.width as f32).to_bits() - 1)) as usize;
                        let yy = sy.max(0.).min(f32::from_bits((image.height as f32).to_bits() - 1)) as usize;
                        let p = &data[(yy * image.width as usize + xx) * 4..][..4];
                        let weight = wx * wy;
                        for k in 0..4 {
                            let sample = p[k] as f32 * (1. / 255.);
                            rgba[k] = weight * sample + rgba[k];
                        }
                        sx += 1.;
                    }
                    sy += 1.;
                }
                if *quality == Quality::High {
                    rgba = rgba.map(|v| v.clamp(0., 1.));
                }
                if *alpha != 1. {
                    rgba = rgba.map(|v| v * alpha.clamp(0., 1.));
                }
            }
            for value in rgba {
                out.extend(value.to_le_bytes());
            }
        }
    }
    Ok(Some(out))
}
