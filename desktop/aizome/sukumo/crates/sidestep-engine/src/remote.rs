//! Optional retained-paint export and replay. Both ends use the same rasterizer;
//! unsupported operations return to ordinary Wayland/video pixels as a whole.
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::sync::{Arc, LazyLock, Mutex};

use crate::{protocol as p, raster, text::fonts};
use aizome_remote_protocol as wire;

fn rect(r: &p::Rect) -> wire::Rect {
    [r.x0, r.y0, r.x1, r.y1]
}
fn local_rect(r: &wire::Rect) -> p::Rect {
    p::Rect::new(r[0], r[1], r[2], r[3])
}
fn transform(t: &tiny_skia::Transform) -> [f32; 6] {
    [t.sx, t.ky, t.kx, t.sy, t.tx, t.ty]
}
fn local_transform(t: &[f32; 6]) -> tiny_skia::Transform {
    tiny_skia::Transform::from_row(t[0], t[1], t[2], t[3], t[4], t[5])
}
fn path(p: &tiny_skia::Path) -> Vec<wire::Segment> {
    p.segments()
        .map(|s| match s {
            tiny_skia::PathSegment::MoveTo(a) => wire::Segment::Move([a.x, a.y]),
            tiny_skia::PathSegment::LineTo(a) => wire::Segment::Line([a.x, a.y]),
            tiny_skia::PathSegment::QuadTo(a, b) => wire::Segment::Quad([a.x, a.y, b.x, b.y]),
            tiny_skia::PathSegment::CubicTo(a, b, c) => wire::Segment::Cubic([a.x, a.y, b.x, b.y, c.x, c.y]),
            tiny_skia::PathSegment::Close => wire::Segment::Close,
        })
        .collect()
}
fn local_path(p: &[wire::Segment]) -> Result<Arc<tiny_skia::Path>, String> {
    let mut b = tiny_skia::PathBuilder::new();
    for s in p {
        match s {
            wire::Segment::Move(a) => b.move_to(a[0], a[1]),
            wire::Segment::Line(a) => b.line_to(a[0], a[1]),
            wire::Segment::Quad(a) => b.quad_to(a[0], a[1], a[2], a[3]),
            wire::Segment::Cubic(a) => b.cubic_to(a[0], a[1], a[2], a[3], a[4], a[5]),
            wire::Segment::Close => b.close(),
        }
    }
    b.finish().map(Arc::new).ok_or_else(|| "invalid retained path".into())
}
fn draw(d: &p::Draw) -> Result<wire::Draw, String> {
    let mut masks = Vec::new();
    if let Some(clips) = &d.mask {
        for c in clips.iter() {
            if c.image.is_some() {
                return Err("image-shaped clips require pixel fallback".into());
            }
            masks.push(wire::Clip { path: path(&c.path), transform: transform(&c.xf), even_odd: c.even_odd, aa: c.aa });
        }
    }
    Ok(wire::Draw {
        transform: transform(&d.xf),
        blend: d.blend as u8,
        aa: d.aa,
        clip: rect(&d.clip),
        masks,
        shadow: d.shadow.as_ref().map(|s| wire::Shadow {
            offset: [s.dx, s.dy],
            blur: s.blur,
            color: s.color,
            only: s.only,
        }),
    })
}
fn local_draw(d: &wire::Draw) -> Result<p::Draw, String> {
    let masks = d
        .masks
        .iter()
        .map(|c| {
            Ok(p::ClipPath {
                path: local_path(&c.path)?,
                xf: local_transform(&c.transform),
                even_odd: c.even_odd,
                aa: c.aa,
                image: None,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(p::Draw {
        xf: local_transform(&d.transform),
        blend: p::Blend::from_raw(d.blend as usize),
        aa: d.aa,
        clip: local_rect(&d.clip),
        mask: (!masks.is_empty()).then(|| masks.into()),
        shadow: d.shadow.as_ref().map(|s| {
            Arc::new(p::ShadowSpec { dx: s.offset[0], dy: s.offset[1], blur: s.blur, color: s.color, only: s.only })
        }),
    })
}
fn paint(p: &p::Paint) -> wire::Paint {
    match p {
        p::Paint::Solid(c) => wire::Paint::Solid(*c),
        p::Paint::Gradient(g) => wire::Paint::Gradient {
            stops: g.stops.clone(),
            start: [g.start.0, g.start.1],
            end: [g.end.0, g.end.1],
            radii: g.radii.map(|r| [r.0, r.1]),
            extend: [g.extend.0, g.extend.1],
        },
    }
}
fn local_paint(p: &wire::Paint) -> p::Paint {
    match p {
        wire::Paint::Solid(c) => p::Paint::Solid(*c),
        wire::Paint::Gradient { stops, start, end, radii, extend } => p::Paint::Gradient(Arc::new(p::GradientSpec {
            stops: stops.clone(),
            start: (start[0], start[1]),
            end: (end[0], end[1]),
            radii: radii.map(|r| (r[0], r[1])),
            extend: (extend[0], extend[1]),
        })),
    }
}

/// Assets are sent once per full repaint, and installed transactionally only
/// after the complete frame can be encoded. A rejected paint cannot leave the
/// receiver referring to assets it never received.
#[derive(Clone, Default)]
pub struct Exporter {
    serial: u32,
    fonts: HashSet<u32>,
    images: HashMap<(u64, u64), u64>,
    asset_bytes: usize,
    next_image: u64,
}
impl Exporter {
    pub fn frame(
        &mut self,
        width: u32,
        height: u32,
        scale: f32,
        full: bool,
        damage: &[p::Rect],
        ops: &[p::Op],
    ) -> Result<Vec<u8>, String> {
        self.batches(width, height, scale, full, &[(damage, ops)])
    }
    pub fn batches(
        &mut self,
        width: u32,
        height: u32,
        scale: f32,
        full: bool,
        batches: &[(&[p::Rect], &[p::Op])],
    ) -> Result<Vec<u8>, String> {
        let mut next = if full { Self { serial: self.serial, ..Self::default() } } else { self.clone() };
        next.serial = next.serial.wrapping_add(1);
        let mut frame = wire::Frame {
            version: wire::VERSION,
            serial: next.serial,
            width,
            height,
            scale,
            full,
            batches: Vec::new(),
            fonts: Vec::new(),
            images: Vec::new(),
        };
        for (damage, ops) in batches {
            let mut batch = wire::Batch { damage: damage.iter().map(rect).collect(), ops: Vec::new() };
            for op in *ops {
                batch.ops.push(next.op(op, &mut frame.fonts, &mut frame.images)?);
            }
            frame.batches.push(batch);
        }
        if next.asset_bytes > wire::MAX_ASSET_BYTES || next.fonts.len() > 256 || next.images.len() > 1024 {
            return Err("retained asset dictionary is full".into());
        }
        let bytes = frame.encode()?;
        *self = next;
        Ok(bytes)
    }
    fn op(
        &mut self,
        op: &p::Op,
        fonts: &mut Vec<wire::Font>,
        images: &mut Vec<wire::Image>,
    ) -> Result<wire::Op, String> {
        Ok(match op {
            p::Op::Fill { rect: r, color } => {
                wire::Op::Fill { rect: rect(r), color: *color, blend: p::Blend::SourceOver as u8 }
            }
            p::Op::FillWith { rect: r, color, blend } => {
                wire::Op::Fill { rect: rect(r), color: *color, blend: *blend as u8 }
            }
            p::Op::FillPath { path: q, even_odd, paint: f, draw: d } => {
                wire::Op::FillPath { path: path(q), even_odd: *even_odd, paint: paint(f), draw: draw(d)? }
            }
            p::Op::StrokePath { path: q, stroke: s, paint: f, draw: d } => wire::Op::StrokePath {
                path: path(q),
                stroke: wire::Stroke { width: s.width, cap: s.cap, join: s.join, miter: s.miter, dash: s.dash.clone() },
                paint: paint(f),
                draw: draw(d)?,
            },
            p::Op::Glyphs(g) => {
                if self.fonts.insert(g.font) {
                    let f = fonts::face_data(g.font).ok_or("missing local font")?;
                    let data = f.font.data.data().to_vec();
                    self.asset_bytes += data.len();
                    fonts.push(wire::Font {
                        id: g.font,
                        data,
                        index: f.font.index,
                        coords: f.coords.to_vec(),
                        embolden: f.embolden,
                        skew: f.skew,
                        stroke: f.stroke,
                    });
                }
                wire::Op::Glyphs {
                    font: g.font,
                    size: g.size,
                    origin: [g.x, g.y],
                    glyphs: g.glyphs.iter().map(|g| (g.id, g.x, g.y)).collect(),
                    color: g.color,
                    clip: rect(&g.clip),
                }
            }
            p::Op::Image { image, src, dst, alpha, quality, tint, tiled, draw: d } => {
                let key = (image.key, image.generation);
                let id = if let Some(id) = self.images.get(&key) {
                    *id
                } else {
                    let (width, height, rgba) = raster::images::remote_pixels(image).ok_or("cannot export image")?;
                    self.asset_bytes += rgba.len();
                    self.next_image += 1;
                    let id = self.next_image;
                    images.push(wire::Image { id, width, height, rgba });
                    self.images.insert(key, id);
                    id
                };
                wire::Op::Image {
                    image: id,
                    src: rect(src),
                    dst: rect(dst),
                    alpha: *alpha,
                    quality: match quality {
                        p::Quality::None => 0,
                        p::Quality::Low => 1,
                        p::Quality::Medium => 2,
                        p::Quality::High => 3,
                    },
                    tint: *tint,
                    tiled: *tiled,
                    draw: draw(d)?,
                }
            }
            p::Op::AlphaMask { mask, dst, clip, color } => wire::Op::AlphaMask {
                width: mask.width(),
                height: mask.height(),
                alpha: mask.alpha().to_vec(),
                dst: rect(dst),
                clip: rect(clip),
                color: *color,
            },
            p::Op::BeginGroup { alpha, draw: d } => wire::Op::BeginGroup { alpha: *alpha, draw: draw(d)? },
            p::Op::EndGroup => wire::Op::EndGroup,
            p::Op::Composite(_) => return Err("Core Animation trees require pixel fallback".into()),
        })
    }
}

// Sukumo's process-wide face registry intentionally retains faces. Remote
// sessions therefore share a separately capped, content-deduplicated registry,
// rather than growing it again on every reconnect or attacker-supplied id.
#[derive(Default)]
struct RemoteFonts {
    entries: HashMap<u64, Vec<u32>>,
    bytes: usize,
    faces: usize,
}
static REMOTE_FONTS: LazyLock<Mutex<RemoteFonts>> = LazyLock::new(Default::default);
fn register_font(f: &wire::Font) -> Result<u32, String> {
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    f.data.hash(&mut hash);
    f.index.hash(&mut hash);
    f.coords.hash(&mut hash);
    f.embolden.hash(&mut hash);
    f.skew.to_bits().hash(&mut hash);
    f.stroke.to_bits().hash(&mut hash);
    let hash = hash.finish();
    let mut registry = REMOTE_FONTS.lock().map_err(|_| "retained font registry unavailable")?;
    if let Some(ids) = registry.entries.get(&hash) {
        for id in ids {
            if let Some(old) = fonts::face_data(*id)
                && old.font.data.data() == f.data.as_slice()
                && old.font.index == f.index
                && old.coords.as_ref() == f.coords
                && old.embolden == f.embolden
                && old.skew == f.skew
                && old.stroke == f.stroke
            {
                return Ok(*id);
            }
        }
    }
    if registry.bytes + f.data.len() > wire::MAX_ASSET_BYTES || registry.faces >= 256 {
        return Err("retained font budget exhausted".into());
    }
    skrifa::FontRef::from_index(&f.data, f.index).map_err(|_| "invalid retained font data")?;
    let data = parley::FontData::new(parley::fontique::Blob::from(f.data.clone()), f.index);
    let id = fonts::register(&data, &f.coords, fonts::Synth { embolden: f.embolden, skew: f.skew, stroke: f.stroke });
    registry.bytes += f.data.len();
    registry.faces += 1;
    registry.entries.entry(hash).or_default().push(id);
    Ok(id)
}

#[derive(Default)]
pub struct Player {
    pub pixels: Vec<u32>,
    pub width: u32,
    pub height: u32,
    pub damage: Vec<[u32; 4]>,
    scale: f32,
    serial: Option<u32>,
    fonts: HashMap<u32, u32>,
    images: HashMap<u64, Arc<p::ImageData>>,
    asset_bytes: usize,
    glyphs: raster::Glyphs,
}
impl Player {
    /// A complete validated message, then only the damaged pixels are drawn.
    /// On any error the caller must show ordinary video until a new full frame.
    pub fn frame(&mut self, bytes: &[u8]) -> Result<(), String> {
        let frame = wire::Frame::parse(bytes)?;
        if !frame.full
            && (self.serial.map(|n| n.wrapping_add(1)) != Some(frame.serial)
                || (frame.width, frame.height, frame.scale) != (self.width, self.height, self.scale))
        {
            return Err("retained delta without matching full state".into());
        }
        if frame.full {
            self.reset();
            self.width = frame.width;
            self.height = frame.height;
            self.scale = frame.scale;
            self.pixels.resize((u64::from(self.width) * u64::from(self.height)) as usize, 0);
        }
        for f in &frame.fonts {
            if self.fonts.contains_key(&f.id) {
                return Err("duplicate retained font asset".into());
            }
            self.asset_bytes += f.data.len();
            self.fonts.insert(f.id, register_font(f)?);
        }
        for image in &frame.images {
            if self.images.contains_key(&image.id) {
                return Err("duplicate retained image asset".into());
            }
            self.asset_bytes += image.rgba.len();
            let key = raster::images::next_key();
            self.images.insert(
                image.id,
                Arc::new(p::ImageData {
                    key,
                    generation: 0,
                    width: image.width,
                    height: image.height,
                    pixels: raster::images::Pixels::Rgba(image.rgba.clone().into()),
                }),
            );
        }
        if self.asset_bytes > wire::MAX_ASSET_BYTES || self.fonts.len() > 256 || self.images.len() > 1024 {
            return Err("retained dictionary budget exceeded".into());
        }
        self.damage.clear();
        for batch in &frame.batches {
            let ops = batch.ops.iter().map(|op| self.op(op)).collect::<Result<Vec<_>, String>>()?;
            let damage: Vec<_> = batch.damage.iter().map(local_rect).collect();
            let mut canvas = raster::Canvas::new(&mut self.pixels, self.width, self.height, 0.0, self.scale);
            self.damage.extend(damage.iter().filter_map(|r| {
                canvas.pixels(r).map(|(x0, y0, x1, y1)| [x0 as u32, y0 as u32, (x1 - x0) as u32, (y1 - y0) as u32])
            }));
            raster::paint(&mut canvas, &mut self.glyphs, &damage, &ops);
        }
        self.serial = Some(frame.serial);
        Ok(())
    }
    pub fn reset(&mut self) {
        raster::images::forget(&self.images.values().map(|i| i.key).collect::<Vec<_>>());
        self.images.clear();
        self.fonts.clear();
        self.asset_bytes = 0;
        self.serial = None;
        self.glyphs = raster::Glyphs::default();
        self.pixels.clear();
        self.damage.clear();
    }
    fn op(&self, op: &wire::Op) -> Result<p::Op, String> {
        Ok(match op {
            wire::Op::Fill { rect: r, color, blend } => {
                p::Op::FillWith { rect: local_rect(r), color: *color, blend: p::Blend::from_raw(*blend as usize) }
            }
            wire::Op::FillPath { path, even_odd, paint, draw } => p::Op::FillPath {
                path: local_path(path)?,
                even_odd: *even_odd,
                paint: local_paint(paint),
                draw: local_draw(draw)?,
            },
            wire::Op::StrokePath { path, stroke: s, paint, draw } => p::Op::StrokePath {
                path: local_path(path)?,
                stroke: Arc::new(p::StrokeSpec {
                    width: s.width,
                    cap: s.cap,
                    join: s.join,
                    miter: s.miter,
                    dash: s.dash.clone(),
                }),
                paint: local_paint(paint),
                draw: local_draw(draw)?,
            },
            wire::Op::Glyphs { font, size, origin, glyphs, color, clip } => p::Op::Glyphs(p::GlyphRun {
                font: *self.fonts.get(font).ok_or("missing retained font asset")?,
                size: *size,
                x: origin[0],
                y: origin[1],
                glyphs: glyphs.iter().map(|(id, x, y)| p::Glyph { id: *id, x: *x, y: *y }).collect::<Vec<_>>().into(),
                color: *color,
                clip: local_rect(clip),
            }),
            wire::Op::Image { image, src, dst, alpha, quality, tint, tiled, draw } => p::Op::Image {
                image: self.images.get(image).ok_or("missing retained image asset")?.clone(),
                src: local_rect(src),
                dst: local_rect(dst),
                alpha: *alpha,
                quality: match quality {
                    0 => p::Quality::None,
                    1 => p::Quality::Low,
                    2 => p::Quality::Medium,
                    _ => p::Quality::High,
                },
                tint: *tint,
                tiled: *tiled,
                draw: local_draw(draw)?,
            },
            wire::Op::AlphaMask { width, height, alpha, dst, clip, color } => p::Op::AlphaMask {
                mask: Arc::new(p::AlphaMask::new(*width, *height, alpha.clone()).ok_or("invalid retained mask")?),
                dst: local_rect(dst),
                clip: local_rect(clip),
                color: *color,
            },
            wire::Op::BeginGroup { alpha, draw } => p::Op::BeginGroup { alpha: *alpha, draw: local_draw(draw)? },
            wire::Op::EndGroup => p::Op::EndGroup,
        })
    }
}
impl Drop for Player {
    fn drop(&mut self) {
        self.reset();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn replay_is_exact_and_deltas_require_contiguous_state() {
        let all = p::Rect::new(0.0, 0.0, 128.0, 96.0);
        let path = tiny_skia::PathBuilder::from_circle(40.0, 40.0, 20.0).unwrap();
        let ops = vec![
            p::Op::FillWith { rect: all, color: [0.2, 0.3, 0.4, 1.0], blend: p::Blend::Copy },
            p::Op::FillPath {
                path: Arc::new(path),
                even_odd: false,
                paint: p::Paint::Solid([0.9, 0.2, 0.1, 0.7]),
                draw: p::Draw {
                    xf: tiny_skia::Transform::identity(),
                    blend: p::Blend::SourceOver,
                    aa: true,
                    clip: all,
                    mask: None,
                    shadow: None,
                },
            },
        ];
        let mut expected = vec![0; 128 * 96];
        let mut glyphs = raster::Glyphs::default();
        raster::paint(&mut raster::Canvas::new(&mut expected, 128, 96, 0.0, 1.0), &mut glyphs, &[all], &ops);
        let mut exporter = Exporter::default();
        let mut player = Player::default();
        player.frame(&exporter.frame(128, 96, 1.0, true, &[all], &ops).unwrap()).unwrap();
        assert_eq!(player.pixels, expected);
        let r = p::Rect::new(10.0, 10.0, 20.0, 20.0);
        let delta =
            exporter.frame(128, 96, 1.0, false, &[r], &[p::Op::Fill { rect: r, color: [0.0, 1.0, 0.0, 1.0] }]).unwrap();
        player.frame(&delta).unwrap();
        assert_eq!(player.damage, vec![[10, 10, 10, 10]]);
        assert_eq!(player.pixels[12 * 128 + 12], raster::premultiplied([0.0, 1.0, 0.0, 1.0]));
        assert_eq!(player.pixels[0], expected[0]);
        assert!(player.frame(&delta).is_err());
        assert!(Player::default().frame(&delta).is_err());
    }
    #[test]
    fn batches_assets_groups_and_fractional_damage_match_local_raster() {
        let all = p::Rect::new(0.0, 0.0, 128.0, 96.0);
        let draw = p::Draw {
            xf: tiny_skia::Transform::identity(),
            blend: p::Blend::SourceOver,
            aa: true,
            clip: all,
            mask: None,
            shadow: None,
        };
        let image = Arc::new(p::ImageData {
            key: raster::images::next_key(),
            generation: 0,
            width: 2,
            height: 2,
            pixels: raster::images::Pixels::Rgba(Arc::from([
                255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 128, 128, 0, 128,
            ])),
        });
        let ops = vec![
            p::Op::FillWith { rect: all, color: [0.15, 0.25, 0.35, 1.0], blend: p::Blend::Copy },
            p::Op::BeginGroup { alpha: 0.6, draw: draw.clone() },
            p::Op::Image {
                image,
                src: p::Rect::new(0.0, 0.0, 2.0, 2.0),
                dst: p::Rect::new(10.25, 15.5, 85.0, 70.0),
                alpha: 0.9,
                quality: p::Quality::High,
                tint: None,
                tiled: false,
                draw: draw.clone(),
            },
            p::Op::EndGroup,
        ];
        for scale in [1.0, 1.25, 2.0] {
            let (width, height) = ((128.0 * scale) as u32, (96.0 * scale) as u32);
            let mut expected = vec![0; (width * height) as usize];
            let mut glyphs = raster::Glyphs::default();
            raster::paint(
                &mut raster::Canvas::new(&mut expected, width, height, 0.0, scale),
                &mut glyphs,
                &[all],
                &ops,
            );
            let mut exporter = Exporter::default();
            let mut player = Player::default();
            let full = exporter.frame(width, height, scale, true, &[all], &ops).unwrap();
            player.frame(&full).unwrap();
            assert_eq!(player.pixels, expected, "full scale={scale}");
            let a = p::Rect::new(5.0, 5.0, 26.0, 26.0);
            let b = p::Rect::new(20.0, 20.0, 40.0, 40.0);
            // Both paints mention the whole canvas. Their distinct, overlapping
            // damage clips must stay distinct and preserve their original order.
            let a_ops = vec![p::Op::Fill { rect: all, color: [1.0, 0.0, 0.0, 0.4] }];
            let b_ops = vec![p::Op::Fill { rect: all, color: [0.0, 1.0, 0.0, 0.8] }];
            let bytes = exporter.batches(width, height, scale, false, &[(&[a], &a_ops), (&[b], &b_ops)]).unwrap();
            player.frame(&bytes).unwrap();
            for (damage, ops) in [(a, &a_ops), (b, &b_ops)] {
                raster::paint(
                    &mut raster::Canvas::new(&mut expected, width, height, 0.0, scale),
                    &mut glyphs,
                    &[damage],
                    ops,
                );
            }
            assert_eq!(player.pixels, expected, "ordered damage scale={scale}");
            let mut bad = wire::Frame::parse(&full).unwrap();
            bad.images.clear();
            assert!(Player::default().frame(&bad.encode().unwrap()).is_err(), "missing assets must fail");
            let repeat = exporter.frame(width, height, scale, false, &[all], &ops).unwrap();
            assert!(wire::Frame::parse(&repeat).unwrap().images.is_empty(), "cached image must not be retransmitted");
        }
    }
}
