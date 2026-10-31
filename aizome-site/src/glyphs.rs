//! Glyphs as outlines. A CJK webfont would cost megabytes for the few dozen
//! kanji the site uses, and the modifier glyphs Aizome prints its shortcuts
//! with (❖ ⇧ ⌥ ⌃) are in no face the site serves, so the generator draws
//! both as inline SVG: kanji from Noto Serif CJK JP (Aizome's kanji face),
//! key glyphs from Noto Sans Symbols 2 first, as Aizome's menus take them,
//! then Noto Sans Symbols and Noto Sans Math. The text itself stays in the
//! page for screen readers, search and copying.

use anyhow::{Context, Result, anyhow, bail};
use std::fmt::Write;
use std::path::Path;
use ttf_parser::{Face, OutlineBuilder, Tag, name_id};

struct FontFile {
    data: Vec<u8>,
    index: u32,
    weight: f32,
}

/// One or more faces, searched in order for each glyph.
pub struct Outliner {
    faces: Vec<FontFile>,
}

struct Pen {
    d: String,
    dx: f32,
    top: f32,
}

impl Pen {
    fn pt(&mut self, x: f32, y: f32) {
        // Font units grow upwards; SVG's grow downwards from the ascender.
        let _ = write!(self.d, "{:.0} {:.0}", x + self.dx, self.top - y);
    }
}

impl OutlineBuilder for Pen {
    fn move_to(&mut self, x: f32, y: f32) {
        self.d.push('M');
        self.pt(x, y);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.d.push('L');
        self.pt(x, y);
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        self.d.push('Q');
        self.pt(x1, y1);
        self.d.push(' ');
        self.pt(x, y);
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.d.push('C');
        self.pt(x1, y1);
        self.d.push(' ');
        self.pt(x2, y2);
        self.d.push(' ');
        self.pt(x, y);
    }
    fn close(&mut self) {
        self.d.push('Z');
    }
}

impl FontFile {
    fn face(&self) -> Result<Face<'_>> {
        let mut face = Face::parse(&self.data, self.index).map_err(|e| anyhow!("{e}"))?;
        if face.is_variable() {
            face.set_variation(Tag::from_bytes(b"wght"), self.weight);
        }
        Ok(face)
    }
}

impl Outliner {
    /// The Japanese face of a Noto Serif CJK collection.
    pub fn japanese(path: &Path, weight: f32) -> Result<Self> {
        let data = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
        let count = ttf_parser::fonts_in_collection(&data).unwrap_or(1);
        for index in 0..count {
            let face = Face::parse(&data, index).map_err(|e| anyhow!("{e}"))?;
            let japanese = face.names().into_iter().any(|n| {
                n.name_id == name_id::FAMILY && n.to_string().is_some_and(|s| s.ends_with(" JP"))
            });
            if japanese {
                return Ok(Outliner {
                    faces: vec![FontFile {
                        data,
                        index,
                        weight,
                    }],
                });
            }
        }
        bail!("{} has no Japanese face", path.display())
    }

    /// Single fonts, searched in the order given.
    pub fn fonts(paths: &[&Path], weight: f32) -> Result<Self> {
        let mut faces = Vec::new();
        for path in paths {
            let data =
                std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
            Face::parse(&data, 0).map_err(|e| anyhow!("{}: {e}", path.display()))?;
            faces.push(FontFile {
                data,
                index: 0,
                weight,
            });
        }
        Ok(Outliner { faces })
    }

    /// `text` drawn as an inline SVG one em tall, coloured by `currentColor`.
    /// Every character comes from the first face that has them all.
    pub fn svg(&self, text: &str, class: &str) -> Result<String> {
        self.outline(text, class, false)
    }

    /// As `svg`, but cropped to the ink, so glyphs of different faces and
    /// sizes (❖ and an arrow) can be drawn as tall as each other.
    pub fn svg_tight(&self, text: &str, class: &str) -> Result<String> {
        self.outline(text, class, true)
    }

    fn outline(&self, text: &str, class: &str, tight: bool) -> Result<String> {
        for file in &self.faces {
            let face = file.face()?;
            if text.chars().all(|c| face.glyph_index(c).is_some()) {
                return Ok(draw(&face, text, class, tight));
            }
        }
        bail!("no face has all of {text}")
    }
}

fn draw(face: &Face, text: &str, class: &str, tight: bool) -> String {
    // The em box (typographic metrics), not the taller line box, so a
    // glyph is as tall as the text beside it.
    let top = face.typographic_ascender().unwrap_or(face.ascender()) as f32;
    let height = top - face.typographic_descender().unwrap_or(face.descender()) as f32;
    let mut pen = Pen {
        d: String::new(),
        dx: 0.0,
        top,
    };
    // The ink's extent, in the SVG's coordinates.
    let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for ch in text.chars() {
        if let Some(gid) = face.glyph_index(ch) {
            if let Some(b) = face.outline_glyph(gid, &mut pen) {
                x0 = x0.min(pen.dx + f32::from(b.x_min));
                x1 = x1.max(pen.dx + f32::from(b.x_max));
                y0 = y0.min(top - f32::from(b.y_max));
                y1 = y1.max(top - f32::from(b.y_min));
            }
            pen.dx += f32::from(face.glyph_hor_advance(gid).unwrap_or(1000));
        }
    }
    // Tight glyphs fit a box 0.72em tall and at most 1em wide; their size
    // is given as attributes, which the security policy allows.
    let (view, size) = if tight && x1 > x0 {
        let (w, h) = (x1 - x0, y1 - y0);
        let (mut ew, mut eh) = (0.72 * w / h, 0.72);
        if ew > 1.0 {
            (ew, eh) = (1.0, h / w);
        }
        (
            format!("{x0:.0} {y0:.0} {w:.0} {h:.0}"),
            format!(r#" width="{ew:.3}em" height="{eh:.3}em""#),
        )
    } else {
        (format!("0 0 {:.0} {height:.0}", pen.dx), String::new())
    };
    format!(
        r#"<svg class="{class}" viewBox="{view}"{size} aria-hidden="true" focusable="false"><path d="{}"/></svg>"#,
        pen.d
    )
}
