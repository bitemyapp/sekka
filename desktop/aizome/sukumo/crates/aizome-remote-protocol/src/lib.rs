//! Versioned, bounded data only. The wire contains drawing operations and assets,
//! never executable code, local paths, resource URLs, or widget event handlers.
use serde::{Deserialize, Serialize};

pub const VERSION: u32 = 1;
pub const MAX_FRAME_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_PIXELS: u64 = 16 * 1024 * 1024;
pub const MAX_OPS: usize = 8192;
pub const MAX_ASSET_BYTES: usize = 64 * 1024 * 1024;
pub type Rect = [f32; 4];
pub type Color = [f32; 4];

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Frame {
    pub version: u32,
    pub serial: u32,
    pub width: u32,
    pub height: u32,
    pub scale: f32,
    pub full: bool,
    pub batches: Vec<Batch>,
    pub fonts: Vec<Font>,
    pub images: Vec<Image>,
    /// Final-scale glyph coverage, cached independently of paint color.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub masks: Vec<Mask>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Batch {
    pub damage: Vec<Rect>,
    pub ops: Vec<Op>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Font {
    pub id: u32,
    #[serde(with = "blob")]
    pub data: Vec<u8>,
    pub index: u32,
    pub coords: Vec<i16>,
    pub embolden: bool,
    pub skew: f32,
    pub stroke: f32,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Image {
    pub id: u64,
    pub width: u32,
    pub height: u32,
    /// Premultiplied RGBA, with exactly width * height * 4 bytes.
    #[serde(with = "blob")]
    pub rgba: Vec<u8>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Mask {
    pub id: u32,
    pub width: u32,
    pub height: u32,
    #[serde(with = "blob")]
    pub alpha: Vec<u8>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Segment {
    Move([f32; 2]),
    Line([f32; 2]),
    Quad([f32; 4]),
    Cubic([f32; 6]),
    Close,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Paint {
    Solid(Color),
    Gradient { stops: Vec<(f32, Color)>, start: [f32; 2], end: [f32; 2], radii: Option<[f32; 2]>, extend: [bool; 2] },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Draw {
    pub transform: [f32; 6],
    /// NSCompositingOperation values 0..=28, as in Sukumo's Blend enum.
    pub blend: u8,
    pub aa: bool,
    pub clip: Rect,
    pub masks: Vec<Clip>,
    pub shadow: Option<Shadow>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Clip {
    pub path: Vec<Segment>,
    pub transform: [f32; 6],
    pub even_odd: bool,
    pub aa: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Shadow {
    pub offset: [f32; 2],
    pub blur: f32,
    pub color: Color,
    pub only: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Stroke {
    pub width: f32,
    pub cap: u8,
    pub join: u8,
    pub miter: f32,
    pub dash: Option<(Vec<f32>, f32)>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Op {
    Fill {
        rect: Rect,
        color: Color,
        blend: u8,
    },
    FillPath {
        path: Vec<Segment>,
        even_odd: bool,
        paint: Paint,
        draw: Draw,
    },
    StrokePath {
        path: Vec<Segment>,
        stroke: Stroke,
        paint: Paint,
        draw: Draw,
    },
    Glyphs {
        font: u32,
        size: f32,
        origin: [f32; 2],
        glyphs: Vec<(u32, f32, f32)>,
        color: Color,
        clip: Rect,
    },
    /// Cached coverage ids and integer physical-pixel top-left positions.
    MaskGlyphs {
        glyphs: Vec<(u32, i32, i32)>,
        color: Color,
        clip: Rect,
    },
    Image {
        image: u64,
        src: Rect,
        dst: Rect,
        alpha: f32,
        quality: u8,
        tint: Option<Color>,
        tiled: bool,
        draw: Draw,
    },
    AlphaMask {
        width: u32,
        height: u32,
        #[serde(with = "blob")]
        alpha: Vec<u8>,
        dst: Rect,
        clip: Rect,
        color: Color,
    },
    BeginGroup {
        alpha: f32,
        draw: Draw,
    },
    EndGroup,
}

fn real(values: &[f32]) -> bool {
    values.iter().all(|v| v.is_finite() && v.abs() <= 65_536.0)
}
fn color(c: &Color) -> bool {
    c.iter().all(|v| v.is_finite() && (0.0..=1.0).contains(v))
}
fn path(p: &[Segment]) -> bool {
    !p.is_empty()
        && p.len() <= 65_536
        && p.iter().all(|s| match s {
            Segment::Move(v) | Segment::Line(v) => real(v),
            Segment::Quad(v) => real(v),
            Segment::Cubic(v) => real(v),
            Segment::Close => true,
        })
}
fn draw(d: &Draw) -> bool {
    d.blend <= 28
        && real(&d.transform)
        && real(&d.clip)
        && d.masks.len() <= 8
        && d.masks.iter().all(|m| path(&m.path) && real(&m.transform))
        && d.shadow.as_ref().is_none_or(|s| real(&s.offset) && color(&s.color) && (0.0..=256.0).contains(&s.blur))
}
fn paint(p: &Paint) -> bool {
    match p {
        Paint::Solid(c) => color(c),
        Paint::Gradient { stops, start, end, radii, .. } => {
            !stops.is_empty()
                && stops.len() <= 256
                && stops.iter().all(|(p, c)| (0.0..=1.0).contains(p) && color(c))
                && real(start)
                && real(end)
                && radii.as_ref().is_none_or(|r| real(r) && r.iter().all(|v| *v >= 0.0))
        }
    }
}
impl Frame {
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() > MAX_FRAME_BYTES {
            return Err("retained frame exceeds byte limit".into());
        }
        let frame: Self = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
        frame.validate()?;
        Ok(frame)
    }
    pub fn encode(&self) -> Result<Vec<u8>, String> {
        self.validate()?;
        let bytes = serde_json::to_vec(self).map_err(|e| e.to_string())?;
        if bytes.len() > MAX_FRAME_BYTES {
            return Err("retained frame exceeds byte limit".into());
        }
        Ok(bytes)
    }
    pub fn validate(&self) -> Result<(), String> {
        let valid = self.version == VERSION
            && self.width > 0
            && self.height > 0
            && self.width <= 16384
            && self.height <= 16384
            && u64::from(self.width) * u64::from(self.height) <= MAX_PIXELS
            && self.scale.is_finite()
            && (0.5..=4.0).contains(&self.scale)
            && !self.batches.is_empty()
            && self.batches.len() <= 64
            && self
                .batches
                .iter()
                .all(|b| !b.damage.is_empty() && b.damage.len() <= 256 && b.damage.iter().all(|r| real(r)))
            && self
                .batches
                .iter()
                .flat_map(|b| &b.ops)
                .map(|op| match op {
                    Op::MaskGlyphs { glyphs, .. } => glyphs.len().max(1),
                    _ => 1,
                })
                .sum::<usize>()
                <= MAX_OPS
            && self.fonts.len() <= 64
            && self.images.len() <= 256
            && self.masks.len() <= MAX_OPS;
        if !valid {
            return Err("invalid retained frame geometry/version/counts".into());
        }
        if self.full
            && !self.batches[0].damage.iter().any(|r| {
                r[0] <= 0.0
                    && r[1] <= 0.0
                    && r[2] >= self.width as f32 / self.scale
                    && r[3] >= self.height as f32 / self.scale
            })
        {
            return Err("full retained frame does not cover its canvas".into());
        }
        let asset_bytes: usize = self
            .fonts
            .iter()
            .map(|f| f.data.len())
            .chain(self.images.iter().map(|i| i.rgba.len()))
            .chain(self.masks.iter().map(|m| m.alpha.len()))
            .sum();
        if asset_bytes > MAX_ASSET_BYTES {
            return Err("retained asset budget exceeded".into());
        }
        for f in &self.fonts {
            if f.data.is_empty()
                || f.data.len() > MAX_FRAME_BYTES
                || f.coords.len() > 64
                || !real(&[f.skew, f.stroke])
                || f.skew.abs() > 89.0
                || !(0.0..=1.0).contains(&f.stroke)
            {
                return Err("invalid retained font".into());
            }
        }
        for i in &self.images {
            let pixels = u64::from(i.width) * u64::from(i.height);
            if pixels == 0 || pixels > MAX_PIXELS || pixels * 4 != i.rgba.len() as u64 {
                return Err("invalid retained image".into());
            }
        }
        for m in &self.masks {
            let pixels = u64::from(m.width) * u64::from(m.height);
            if pixels == 0 || pixels > 4 * 1024 * 1024 || pixels != m.alpha.len() as u64 {
                return Err("invalid retained glyph mask".into());
            }
        }
        for batch in &self.batches {
            let mut groups = 0u32;
            for op in &batch.ops {
                let valid = match op {
                    Op::Fill { rect, color: c, blend } => real(rect) && color(c) && *blend <= 28,
                    Op::FillPath { path: p, paint: fill, draw: d, .. } => path(p) && paint(fill) && draw(d),
                    Op::StrokePath { path: p, stroke: s, paint: fill, draw: d } => {
                        path(p)
                            && paint(fill)
                            && draw(d)
                            && (0.0..=4096.0).contains(&s.width)
                            && s.cap <= 2
                            && s.join <= 2
                            && (0.0..=4096.0).contains(&s.miter)
                            && s.dash.as_ref().is_none_or(|(v, offset)| {
                                v.len() <= 256 && real(v) && v.iter().all(|v| *v >= 0.0) && real(&[*offset])
                            })
                    }
                    Op::Glyphs { size, origin, glyphs, color: c, clip, .. } => {
                        (0.0..=1024.0).contains(size)
                            && real(origin)
                            && glyphs.len() <= 65_536
                            && glyphs.iter().all(|(id, x, y)| *id <= 65_535 && real(&[*x, *y]))
                            && color(c)
                            && real(clip)
                    }
                    Op::MaskGlyphs { glyphs, color: c, clip } => {
                        glyphs
                            .iter()
                            .all(|(_, x, y)| (-262_144..=262_144).contains(x) && (-262_144..=262_144).contains(y))
                            && color(c)
                            && real(clip)
                    }
                    Op::Image { src, dst, alpha, quality, tint, draw: d, .. } => {
                        real(src)
                            && real(dst)
                            && (0.0..=1.0).contains(alpha)
                            && *quality <= 3
                            && tint.as_ref().is_none_or(color)
                            && draw(d)
                    }
                    Op::AlphaMask { width, height, alpha, dst, clip, color: c } => {
                        let pixels = u64::from(*width) * u64::from(*height);
                        pixels > 0
                            && pixels <= 4 * 1024 * 1024
                            && pixels == alpha.len() as u64
                            && real(dst)
                            && real(clip)
                            && color(c)
                    }
                    Op::BeginGroup { alpha, draw: d } => {
                        groups += 1;
                        groups <= 8 && (0.0..=1.0).contains(alpha) && draw(d)
                    }
                    Op::EndGroup => {
                        if groups == 0 {
                            false
                        } else {
                            groups -= 1;
                            true
                        }
                    }
                };
                if !valid {
                    return Err("invalid retained drawing operation".into());
                }
            }
            if groups != 0 {
                return Err("unclosed retained drawing group".into());
            }
        }
        Ok(())
    }
}

mod blob {
    use base64::{Engine, engine::general_purpose::STANDARD};
    use serde::{Deserialize, Deserializer, Serializer};
    pub fn serialize<S: Serializer>(bytes: &[u8], serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&STANDARD.encode(bytes))
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<u8>, D::Error> {
        let s = String::deserialize(deserializer)?;
        if s.len() > super::MAX_FRAME_BYTES * 4 / 3 + 4 {
            return Err(serde::de::Error::custom("asset exceeds limit"));
        }
        STANDARD.decode(s).map_err(serde::de::Error::custom)
    }
}

#[cfg(feature = "client")]
pub mod client {
    use wayland_client;
    use wayland_client::protocol::*;
    pub mod __interfaces {
        use wayland_client::backend as wayland_backend;
        use wayland_client::protocol::__interfaces::*;
        wayland_scanner::generate_interfaces!("protocol/remote-paint.xml");
    }
    use self::__interfaces::*;
    wayland_scanner::generate_client_code!("protocol/remote-paint.xml");
}
#[cfg(feature = "server")]
pub mod server {
    use wayland_server;
    use wayland_server::protocol::*;
    pub mod __interfaces {
        use wayland_server::backend as wayland_backend;
        use wayland_server::protocol::__interfaces::*;
        wayland_scanner::generate_interfaces!("protocol/remote-paint.xml");
    }
    use self::__interfaces::*;
    wayland_scanner::generate_server_code!("protocol/remote-paint.xml");
}

#[cfg(test)]
mod tests {
    use super::*;
    fn frame() -> Frame {
        Frame {
            version: VERSION,
            serial: 1,
            width: 1920,
            height: 1080,
            scale: 1.0,
            full: true,
            fonts: vec![],
            images: vec![],
            masks: vec![],
            batches: vec![Batch {
                damage: vec![[0.0, 0.0, 1920.0, 1080.0]],
                ops: vec![Op::Fill { rect: [0.0, 0.0, 1920.0, 1080.0], color: [1.0, 1.0, 1.0, 1.0], blend: 1 }],
            }],
        }
    }
    #[test]
    fn rejects_unbounded_or_inconsistent_retained_data() {
        let f = frame();
        assert_eq!(Frame::parse(&f.encode().unwrap()).unwrap().width, 1920);
        let mut bad = f.clone();
        bad.width = u32::MAX;
        assert!(bad.validate().is_err());
        let mut bad = f.clone();
        bad.scale = f32::NAN;
        assert!(bad.validate().is_err());
        let mut bad = f.clone();
        bad.batches[0].ops.push(Op::EndGroup);
        assert!(bad.validate().is_err());
        let mut bad = f.clone();
        bad.images.push(Image { id: 1, width: 10, height: 10, rgba: vec![0; 4] });
        assert!(bad.validate().is_err());
        let mut bad = f.clone();
        bad.version = VERSION + 1;
        assert!(bad.validate().is_err());
        let mut bad = f.clone();
        bad.masks.push(Mask { id: 1, width: 3, height: 2, alpha: vec![0; 5] });
        assert!(bad.validate().is_err());
        let mut bad = f.clone();
        bad.batches[0].ops.push(Op::MaskGlyphs { glyphs: vec![(1, 0, 0); MAX_OPS], color: [1.0; 4], clip: [0.0; 4] });
        assert!(bad.validate().is_err(), "expanded glyph work must stay bounded");
        assert!(Frame::parse(b"{\"version\":1}").is_err());
    }
}
