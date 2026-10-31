//! Isolated GPU scene preparation. Complete preflight precedes device work.
//! All pixel buffers here are reusable glyph/sprite/small-icon assets, never
//! a software-rendered terminal or window. Unsupported ops fail the whole job.
#![forbid(unsafe_code)]
use crate::{
    protocol::{Blend, Op, Paint, Rect},
    raster::{self, Canvas, Glyphs},
    text::{self, GlyphImage, GlyphPixels},
};
use std::{
    collections::HashMap,
    hash::{Hash, Hasher},
    sync::Arc,
};

#[path = "gpu_scene_samples.rs"]
mod samples;

const MAX_OPS: usize = 65_536;
const MAX_COMMANDS: usize = 131_072;
const MAX_ASSET: usize = 1 << 20;
const MAX_FRAME_ASSETS: usize = 32 << 20;
const MAX_CACHE: usize = 16 << 20;
const MAX_SMALL_PIXELS: usize = 65_536;
const MAX_TILE_INDICES: usize = 4 << 20;
pub const TILE: u32 = 16;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Fallback {
    Geometry,
    Unsupported(&'static str),
    Limit,
    IdentityExhausted,
}
#[derive(Debug)]
pub struct Asset {
    pub id: u64,
    pub width: u32,
    pub height: u32,
    pub format: u32,
    pub data: Arc<[u8]>,
}
#[derive(Clone, Debug)]
pub struct Command {
    pub bounds: [u32; 4],
    /// 0 solid lerp, 1 replacement, 2 mask lerp, 3 premultiplied color over.
    pub kind: u32,
    pub solid: u32,
    pub alpha: u32,
    pub asset: Option<usize>,
    pub origin: [i32; 2],
}
#[derive(Debug)]
pub struct Scene {
    pub width: u32,
    pub height: u32,
    pub commands: Vec<Command>,
    pub assets: Vec<Arc<Asset>>,
    pub tiles: Vec<[u32; 2]>,
    pub indices: Vec<u32>,
}
enum Source {
    Glyph { _image: Arc<GlyphImage> },
    Mask { _mask: Arc<raster::masks::AlphaMask> },
    Small,
}
struct Cached {
    asset: Arc<Asset>,
    _source: Source,
}
#[derive(Default)]
pub struct Prepare {
    glyphs: Glyphs,
    cache: HashMap<(u8, usize), Cached>,
    bytes: usize,
    next_id: u64,
}

impl Prepare {
    fn asset(
        &mut self,
        key: (u8, usize),
        source: Source,
        width: u32,
        height: u32,
        format: u32,
        data: impl FnOnce() -> Vec<u8>,
    ) -> Result<Arc<Asset>, Fallback> {
        if let Some(entry) = self.cache.get(&key) {
            return Ok(entry.asset.clone());
        }
        let size = (width as usize)
            .checked_mul(height as usize)
            .and_then(|n| {
                n.checked_mul(match format {
                    0 => 1,
                    1 => 4,
                    2 => 16,
                    _ => 0,
                })
            })
            .ok_or(Fallback::Limit)?;
        if size == 0 || size > MAX_ASSET {
            return Err(Fallback::Limit);
        }
        let data = data();
        if data.len() != size {
            return Err(Fallback::Geometry);
        }
        if self.bytes.checked_add(size).is_none_or(|n| n > MAX_CACHE) || self.cache.len() >= 4096 {
            self.cache.clear();
            self.bytes = 0;
        }
        self.next_id = self.next_id.checked_add(1).ok_or(Fallback::IdentityExhausted)?;
        let asset = Arc::new(Asset { id: self.next_id, width, height, format, data: data.into() });
        self.bytes += size;
        self.cache.insert(key, Cached { asset: asset.clone(), _source: source });
        Ok(asset)
    }
    fn small(&mut self, width: u32, height: u32, data: Vec<u8>) -> Result<Arc<Asset>, Fallback> {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        (width, height, &data).hash(&mut h);
        let key = (2, h.finish() as usize);
        if let Some(old) = self.cache.get(&key) {
            if old.asset.width == width && old.asset.height == height && &*old.asset.data == data {
                return Ok(old.asset.clone());
            }
            self.bytes -= old.asset.data.len();
            self.cache.remove(&key);
        }
        self.asset(key, Source::Small, width, height, 1, || data)
    }
    pub fn prepare(&mut self, width: u32, height: u32, scale: f32, ops: &[Op]) -> Result<Scene, Fallback> {
        if width == 0
            || height == 0
            || width > 8192
            || height > 8192
            || !scale.is_finite()
            || !(0.0625..=16.).contains(&scale)
        {
            return Err(Fallback::Geometry);
        }
        if ops.len() > MAX_OPS {
            return Err(Fallback::Limit);
        }
        // Full structural preflight. Preparation may still reject a byte budget;
        // neither path submits any GPU command or presents a partial frame.
        let mut glyph_count = 0usize;
        for op in ops {
            preflight(op, scale)?;
            if let Op::Glyphs(run) = op {
                glyph_count = glyph_count.checked_add(run.glyphs.len()).ok_or(Fallback::Limit)?;
                if glyph_count > MAX_COMMANDS {
                    return Err(Fallback::Limit);
                }
            }
        }
        let canvas = Canvas::new(&mut [], width, height, 0., scale);
        let clip = Rect::new(0., 0., width as f32 / scale, height as f32 / scale);
        let mut scene =
            Scene { width, height, commands: Vec::new(), assets: Vec::new(), tiles: Vec::new(), indices: Vec::new() };
        let mut assets = HashMap::new();
        let mut asset_bytes = 0usize;
        for op in ops {
            match op {
                Op::Fill { rect, color }
                | Op::FillWith { rect, color, blend: Blend::SourceOver | Blend::Highlight } => {
                    if let Some(bounds) = pixels(&canvas, &rect.intersect(&clip)) {
                        scene.push(Command {
                            bounds,
                            kind: 0,
                            solid: solid(*color),
                            alpha: alpha(*color),
                            asset: None,
                            origin: [0, 0],
                        })?;
                    }
                }
                Op::FillWith { rect, color, blend } => {
                    if let Some(bounds) = pixels(&canvas, &rect.intersect(&clip)) {
                        scene.push(Command {
                            bounds,
                            kind: 1,
                            solid: if *blend == Blend::Clear {
                                0
                            } else {
                                u32::from_le_bytes(raster::premultiplied(*color).to_ne_bytes())
                            },
                            alpha: 255,
                            asset: None,
                            origin: [0, 0],
                        })?;
                    }
                }
                Op::Glyphs(run) => {
                    let mut prepared = Vec::new();
                    text::visit_glyphs(&canvas, &mut self.glyphs, run, &run.clip.intersect(&clip), |image, x, y| {
                        prepared.push((image, x, y))
                    });
                    for (image, x, y) in prepared {
                        let key = (0, Arc::as_ptr(&image) as usize);
                        let color = matches!(image.pixels(), GlyphPixels::Color(_));
                        let asset = self.asset(
                            key,
                            Source::Glyph { _image: image.clone() },
                            image.width(),
                            image.height(),
                            u32::from(color),
                            || match image.pixels() {
                                GlyphPixels::Mask(p) => p.to_vec(),
                                GlyphPixels::Color(p) => p.iter().flat_map(|p| p.to_ne_bytes()).collect(),
                            },
                        )?;
                        scene.place(
                            asset,
                            [x, y],
                            pixels(&canvas, &run.clip.intersect(&clip)),
                            solid(run.color),
                            alpha(run.color),
                            &mut assets,
                            &mut asset_bytes,
                        )?;
                    }
                }
                Op::AlphaMask { mask, dst, clip: draw_clip, color } => {
                    let left = (dst.x0 * scale - 0.5).ceil() as i32;
                    let top = (dst.y0 * scale - 0.5).ceil() as i32;
                    let key = (1, Arc::as_ptr(mask) as usize);
                    let asset =
                        self.asset(key, Source::Mask { _mask: mask.clone() }, mask.width(), mask.height(), 0, || {
                            mask.alpha().to_vec()
                        })?;
                    scene.place(
                        asset,
                        [left, top],
                        pixels(&canvas, &dst.intersect(draw_clip).intersect(&clip)),
                        solid(*color),
                        alpha(*color),
                        &mut assets,
                        &mut asset_bytes,
                    )?;
                }
                Op::Image { .. } | Op::StrokePath { .. } | Op::FillPath { .. } => {
                    // Small icons and antialiased path artwork only. Their bounded
                    // source asset is cached; destination/window pixels are never
                    // read or rendered by this preparation path.
                    let region = small_region(&canvas, &clip, op).ok_or(Fallback::Geometry)?;
                    let (x, y, w, h) = (region[0], region[1], region[2] - region[0], region[3] - region[1]);
                    let count = (w as usize).checked_mul(h as usize).ok_or(Fallback::Limit)?;
                    if count > MAX_SMALL_PIXELS {
                        return Err(Fallback::Limit);
                    }
                    if let Some(data) = samples::filtered(&canvas, op, region)? {
                        let mut hash = std::collections::hash_map::DefaultHasher::new();
                        (w, h, &data).hash(&mut hash);
                        let key = (4, hash.finish() as usize);
                        if let Some(old) = self.cache.get(&key) {
                            if old.asset.width != w || old.asset.height != h || &*old.asset.data != data {
                                self.bytes -= old.asset.data.len();
                                self.cache.remove(&key);
                            }
                        }
                        let asset = self.asset(key, Source::Small, w, h, 2, || data)?;
                        scene.place(
                            asset,
                            [x as i32, y as i32],
                            Some(region),
                            0,
                            255,
                            &mut assets,
                            &mut asset_bytes,
                        )?;
                        continue;
                    }
                    let mut px = vec![0u32; count];
                    let mut target = Canvas::new(&mut px, w, h, 0., scale);
                    target.x0 = x as i32;
                    target.y0 = y as i32;
                    raster::paint(&mut target, &mut self.glyphs, &[clip], std::slice::from_ref(op));
                    let (asset, tint) = match op {
                        Op::StrokePath { paint: Paint::Solid(c), .. } | Op::FillPath { paint: Paint::Solid(c), .. } => {
                            let data: Vec<_> = px.iter().map(|p| p.to_ne_bytes()[3]).collect();
                            let mut hash = std::collections::hash_map::DefaultHasher::new();
                            (w, h, &data).hash(&mut hash);
                            let key = (3, hash.finish() as usize);
                            if let Some(old) = self.cache.get(&key) {
                                if old.asset.width != w || old.asset.height != h || &*old.asset.data != data {
                                    self.bytes -= old.asset.data.len();
                                    self.cache.remove(&key);
                                }
                            }
                            (self.asset(key, Source::Small, w, h, 0, || data)?, solid(*c))
                        }
                        _ => (self.small(w, h, px.iter().flat_map(|p| p.to_ne_bytes()).collect())?, 0),
                    };
                    let path_mask = asset.format == 0;
                    let before = scene.commands.len();
                    scene.place(asset, [x as i32, y as i32], Some(region), tint, 255, &mut assets, &mut asset_bytes)?;
                    if path_mask && scene.commands.len() > before {
                        scene.commands.last_mut().unwrap().kind = 4;
                    }
                }
                _ => return Err(Fallback::Unsupported("operation")),
            }
        }
        scene.index_tiles()?;
        Ok(scene)
    }
}

fn alpha(c: [f32; 4]) -> u32 {
    (c[3].clamp(0., 1.) * 255. + 0.5) as u32
}
fn solid(c: [f32; 4]) -> u32 {
    u32::from_le_bytes(raster::premultiplied([c[0], c[1], c[2], 1.]).to_ne_bytes())
}
fn pixels(canvas: &Canvas, r: &Rect) -> Option<[u32; 4]> {
    canvas.pixels(r).map(|(a, b, c, d)| [a as u32, b as u32, c as u32, d as u32])
}
fn preflight(op: &Op, scale: f32) -> Result<(), Fallback> {
    let valid = match op {
        Op::Fill { rect, color } | Op::FillWith { rect, color, .. } => finite_rect(rect) && finite_color(color),
        Op::Glyphs(run) => {
            finite_rect(&run.clip)
                && finite_color(&run.color)
                && finite_coord(run.x)
                && finite_coord(run.y)
                && run.glyphs.iter().all(|g| finite_coord(g.x) && finite_coord(g.y))
        }
        Op::AlphaMask { dst, clip, color, .. } => finite_rect(dst) && finite_rect(clip) && finite_color(color),
        Op::Image { src, dst, draw, alpha, tint, .. } => {
            finite_rect(src)
                && finite_rect(dst)
                && finite_rect(&draw.clip)
                && alpha.is_finite()
                && tint.as_ref().is_none_or(finite_color)
        }
        Op::StrokePath { draw, paint: Paint::Solid(c), stroke, .. } => {
            finite_rect(&draw.clip)
                && finite_color(c)
                && stroke.width.is_finite()
                && stroke.miter.is_finite()
                && stroke.dash.as_ref().is_none_or(|d| d.1.is_finite() && d.0.iter().all(|v| v.is_finite()))
        }
        Op::FillPath { draw, paint: Paint::Solid(c), .. } => finite_rect(&draw.clip) && finite_color(c),
        _ => false,
    };
    if !valid {
        return Err(Fallback::Geometry);
    }
    match op {
        Op::Fill { color, .. }
        | Op::FillWith { color, blend: Blend::SourceOver | Blend::Highlight | Blend::Copy | Blend::Clear, .. }
            if color.iter().all(|v| v.is_finite()) =>
        {
            Ok(())
        }
        Op::Glyphs(r) if r.size.is_finite() && r.size * scale <= 512. && r.glyphs.len() <= MAX_COMMANDS => Ok(()),
        Op::AlphaMask { mask, dst, .. }
            if ((dst.x1 as f64 - dst.x0 as f64) * scale as f64 - mask.width() as f64).abs() < 0.001
                && ((dst.y1 as f64 - dst.y0 as f64) * scale as f64 - mask.height() as f64).abs() < 0.001 =>
        {
            Ok(())
        }
        Op::Image { image, draw, tiled: false, .. }
            if image.width <= 512
                && image.height <= 512
                && simple(draw)
                && matches!(image.pixels, raster::images::Pixels::Rgba(_)) =>
        {
            Ok(())
        }
        Op::StrokePath { path, draw, paint: Paint::Solid(c), .. }
        | Op::FillPath { path, draw, paint: Paint::Solid(c), .. }
            if simple(draw) && c[3] == 1. && path.points().len() <= 8192 =>
        {
            Ok(())
        }
        _ => Err(Fallback::Unsupported("operation/transform/format")),
    }
}
fn finite_coord(v: f32) -> bool {
    v.is_finite() && v.abs() <= 1_000_000.
}
fn finite_rect(r: &Rect) -> bool {
    [r.x0, r.y0, r.x1, r.y1].into_iter().all(finite_coord)
}
fn finite_color(c: &[f32; 4]) -> bool {
    c.iter().all(|v| v.is_finite())
}
fn simple(draw: &crate::protocol::Draw) -> bool {
    draw.blend == Blend::SourceOver && draw.mask.is_none() && draw.shadow.is_none() && draw.xf.is_finite()
}
fn small_region(canvas: &Canvas, clip: &Rect, op: &Op) -> Option<[u32; 4]> {
    let (draw, bounds, outset) = match op {
        Op::Image { draw, dst, .. } => (draw, tiny_skia::Rect::from_ltrb(dst.x0, dst.y0, dst.x1, dst.y1)?, 1.),
        Op::FillPath { draw, path, .. } => (draw, path.bounds(), 1.),
        Op::StrokePath { draw, path, stroke, .. } => {
            let scale = canvas.transform().pre_concat(draw.xf);
            let factor = scale.sx.hypot(scale.ky).max(scale.kx.hypot(scale.sy));
            let join = if stroke.join == 0 { stroke.miter.max(1.) } else { 1. };
            let cap = if stroke.cap == 2 { std::f32::consts::SQRT_2 } else { 1. };
            (draw, path.bounds(), (stroke.width * 0.5 * join.max(cap) * factor).max(0.5) + 1.)
        }
        _ => return None,
    };
    raster::ops::region(canvas, clip, draw, bounds, outset).map(|(a, b, c, d)| [a as u32, b as u32, c as u32, d as u32])
}
impl Scene {
    fn push(&mut self, c: Command) -> Result<(), Fallback> {
        if self.commands.len() >= MAX_COMMANDS {
            return Err(Fallback::Limit);
        }
        self.commands.push(c);
        Ok(())
    }
    #[allow(clippy::too_many_arguments)]
    fn place(
        &mut self,
        asset: Arc<Asset>,
        origin: [i32; 2],
        clip: Option<[u32; 4]>,
        solid: u32,
        alpha: u32,
        ids: &mut HashMap<u64, usize>,
        bytes: &mut usize,
    ) -> Result<(), Fallback> {
        let Some(c) = clip else {
            return Ok(());
        };
        let x = origin[0] as i64;
        let y = origin[1] as i64;
        let b = [
            x.max(c[0] as i64),
            y.max(c[1] as i64),
            (x + asset.width as i64).min(c[2] as i64),
            (y + asset.height as i64).min(c[3] as i64),
        ];
        if b[0] >= b[2] || b[1] >= b[3] {
            return Ok(());
        }
        let id = if let Some(i) = ids.get(&asset.id) {
            *i
        } else {
            *bytes = bytes.checked_add(asset.data.len()).ok_or(Fallback::Limit)?;
            if *bytes > MAX_FRAME_ASSETS {
                return Err(Fallback::Limit);
            }
            let i = self.assets.len();
            ids.insert(asset.id, i);
            self.assets.push(asset.clone());
            i
        };
        self.push(Command {
            bounds: b.map(|n| n as u32),
            kind: match asset.format {
                0 => 2,
                1 => 3,
                2 => 5,
                _ => unreachable!(),
            },
            solid,
            alpha,
            asset: Some(id),
            origin,
        })
    }
    fn index_tiles(&mut self) -> Result<(), Fallback> {
        let cols = self.width.div_ceil(TILE);
        let rows = self.height.div_ceil(TILE);
        let mut lists = vec![Vec::<u32>::new(); (cols * rows) as usize];
        let mut total = 0usize;
        for (i, c) in self.commands.iter().enumerate() {
            let [a, b, x, y] = c.bounds;
            let cells = ((x - 1) / TILE - a / TILE + 1) as usize * ((y - 1) / TILE - b / TILE + 1) as usize;
            total = total.checked_add(cells).ok_or(Fallback::Limit)?;
            if total > MAX_TILE_INDICES {
                return Err(Fallback::Limit);
            }
            for ty in b / TILE..=(y - 1) / TILE {
                for tx in a / TILE..=(x - 1) / TILE {
                    lists[(ty * cols + tx) as usize].push(i as u32);
                }
            }
        }
        self.indices.reserve(total);
        self.tiles.reserve(lists.len());
        for list in lists {
            self.tiles.push([self.indices.len() as u32, list.len() as u32]);
            self.indices.extend(list);
        }
        Ok(())
    }
    /// Independent scalar interpreter of the GPU commands, for exact comparison
    /// with the original software renderer before any hardware driver call.
    pub fn interpret(&self) -> Vec<u8> {
        let mut out = vec![0u8; self.width as usize * self.height as usize * 4];
        for c in &self.commands {
            let [x0, y0, x1, y1] = c.bounds;
            for y in y0..y1 {
                for x in x0..x1 {
                    let index = ((y * self.width + x) * 4) as usize;
                    let d: &mut [u8] = &mut out[index..index + 4];
                    let mut color = c.solid.to_le_bytes();
                    let mut a = c.alpha;
                    if let Some(i) = c.asset {
                        let asset = &self.assets[i];
                        let j = ((y as i64 - c.origin[1] as i64) * asset.width as i64 + x as i64 - c.origin[0] as i64)
                            as usize;
                        if c.kind == 5 {
                            let p = &asset.data[j * 16..j * 16 + 16];
                            let mut s = [0f32; 4];
                            for (k, chunk) in p.chunks_exact(4).enumerate() {
                                s[k] = f32::from_le_bytes(chunk.try_into().unwrap());
                            }
                            for k in 0..4 {
                                let dst = d[k] as f32 * (1. / 255.);
                                let value = dst * (1. - s[3]) + s[k];
                                d[k] = (value.clamp(0., 1.) * 255.).round_ties_even() as u8;
                            }
                            continue;
                        }
                        if c.kind == 2 || c.kind == 4 {
                            let cov = asset.data[j] as u32;
                            a = if a == 255 { cov } else { (cov * a * 257 + 32896) >> 16 };
                        } else {
                            color.copy_from_slice(&asset.data[j * 4..j * 4 + 4]);
                            if a != 255 {
                                color = color.map(|v| ((v as u32 * a + 127) / 255) as u8);
                            }
                        }
                    }
                    for k in 0..4 {
                        d[k] = match c.kind {
                            0 | 2 => ((d[k] as u32 * (255 - a) + color[k] as u32 * a + 127) / 255) as u8,
                            1 => color[k],
                            3 => (color[k] as u32 + (d[k] as u32 * (255 - color[3] as u32) + 127) / 255).min(255) as u8,
                            4 => ((d[k] as u32 * (255 - a) + color[k] as u32 * a + 255) >> 8) as u8,
                            _ => unreachable!(),
                        };
                    }
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        protocol::{Draw, Quality},
        raster::images::{ImageData, Pixels, next_key},
    };
    fn exact(ops: &[Op], scale: f32) {
        let (w, h) = (64, 64);
        let mut px = vec![0u32; (w * h) as usize];
        let mut canvas = Canvas::new(&mut px, w, h, 0., scale);
        raster::paint(
            &mut canvas,
            &mut Glyphs::default(),
            &[Rect::new(0., 0., w as f32 / scale, h as f32 / scale)],
            ops,
        );
        let original: Vec<_> = px.iter().flat_map(|p| p.to_ne_bytes()).collect();
        let scene = Prepare::default().prepare(w, h, scale, ops).unwrap();
        let lowered = scene.interpret();
        assert_eq!(original.iter().zip(&lowered).position(|(a, b)| a != b), None, "scale{scale}");
    }
    #[test]
    fn ordered_mask_fill_replacement_negative_clip_matches_original() {
        let mask = Arc::new(raster::masks::AlphaMask::new(7, 5, (0..35).map(|n| (n * 37) as u8).collect()).unwrap());
        for scale in [1., 1.25, 2.] {
            for origin in [-2.5, -0.5, 0.5, 4.499, 17.] {
                for opacity in [0., 0.1, 0.5, 1.] {
                    let dst = Rect::new(origin / scale, origin / scale, (origin + 7.) / scale, (origin + 5.) / scale);
                    exact(
                        &[
                            Op::Fill { rect: Rect::new(0., 0., 64., 64.), color: [0.13, 0.72, 0.49, 1.] },
                            Op::AlphaMask {
                                mask: mask.clone(),
                                dst,
                                clip: Rect::new(1.5, 0., 30., 32.),
                                color: [0.87, 0.14, 0.3, opacity],
                            },
                            Op::FillWith {
                                rect: Rect::new(12.4, 0.2, 15.5, 13.),
                                color: [0.1, 0.2, 0.3, 0.4],
                                blend: Blend::Copy,
                            },
                            Op::FillWith { rect: Rect::new(13., 4., 14., 5.), color: [0.; 4], blend: Blend::Clear },
                        ],
                        scale,
                    );
                }
            }
        }
    }
    #[test]
    fn filtered_icons_keep_unquantized_samples_until_composition() {
        let data: Vec<_> = (0..64u32)
            .flat_map(|i| {
                let a = ((i * 29 + 17) % 256) as u8;
                [a / 2, a / 3, a, a]
            })
            .collect();
        let image = Arc::new(ImageData {
            key: next_key(),
            generation: 0,
            width: 8,
            height: 8,
            pixels: Pixels::Rgba(data.into()),
        });
        for scale in [1., 1.25, 2.] {
            for quality in [Quality::Medium, Quality::High] {
                for shift in [-2.25, 0.1, 6.5] {
                    for opacity in [0.3, 1.] {
                        let draw = Draw {
                            clip: Rect::new(0., 0., 64., 64.),
                            xf: tiny_skia::Transform::from_translate(shift, 2.25),
                            blend:Blend::SourceOver,
                            aa:true,
                            mask:None,
                            shadow:None,
                        };
                        exact(
                            &[
                                Op::Fill { rect: Rect::new(0., 0., 64., 64.), color: [0.13, 0.72, 0.49, 1.] },
                                Op::Image {
                                    image: image.clone(),
                                    src: Rect::new(0., 0., 8., 8.),
                                    dst: Rect::new(0., 0., 9.3, 10.4),
                                    alpha: opacity,
                                    quality,
                                    tint: Some([0.2, 0.6, 0.8, 0.7]),
                                    tiled: false,
                                    draw,
                                },
                            ],
                            scale,
                        );
                    }
                }
            }
        }
    }
    #[test]
    fn retained_asset_identity_survives_cache_clear_and_exhaustion_is_explicit() {
        let mut p = Prepare::default();
        let old = p.asset((2, 1), Source::Small, 1, 1, 0, || vec![42]).unwrap();
        p.cache.clear();
        p.bytes = 0;
        let new = p.asset((2, 1), Source::Small, 1, 1, 0, || vec![43]).unwrap();
        assert_ne!(old.id, new.id);
        assert_eq!(&*old.data, &[42]);
        assert_eq!(&*new.data, &[43]);
        p.next_id = u64::MAX;
        assert!(matches!(p.asset((2, 2), Source::Small, 1, 1, 0, || vec![44]), Err(Fallback::IdentityExhausted)));
        assert!(matches!(
            p.asset((2, 3), Source::Small, u32::MAX, u32::MAX, 2, || panic!("rejected before allocation")),
            Err(Fallback::Limit)
        ));
    }
    #[test]
    fn whole_scene_rejects_unsupported_or_nonfinite_before_allocating_assets() {
        let mut p = Prepare::default();
        for ops in
            [vec![Op::EndGroup], vec![Op::Fill { rect: Rect::new(0., 0., 3., 3.), color: [f32::NAN, 0., 0., 1.] }]]
        {
            assert!(p.prepare(64, 64, 1., &ops).is_err());
            assert_eq!(p.bytes, 0);
            assert_eq!(p.next_id, 0);
        }
        assert!(p.prepare(u32::MAX, 64, 1., &[]).is_err());
    }
    #[test]
    fn tile_indices_are_bounded_and_preserve_order() {
        let ops: Vec<_> = (0..17).map(|_| Op::Fill { rect: Rect::new(0., 0., 8192., 8192.), color: [1.; 4] }).collect();
        assert!(matches!(Prepare::default().prepare(8192, 8192, 1., &ops), Err(Fallback::Limit)));
        let scene = Prepare::default()
            .prepare(
                32,
                16,
                1.,
                &[
                    Op::Fill { rect: Rect::new(0., 0., 32., 16.), color: [0.; 4] },
                    Op::Fill { rect: Rect::new(16., 0., 32., 16.), color: [1.; 4] },
                ],
            )
            .unwrap();
        assert_eq!(scene.tiles, [[0, 1], [1, 2]]);
        assert_eq!(scene.indices, [0, 0, 1]);
    }
}
