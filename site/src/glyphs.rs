//! Kanji as outlines. A CJK webfont would cost megabytes for the dozen
//! characters the site uses, so the generator draws them from Noto Serif
//! CJK JP (the face of the lockup's 雪花) as inline SVG. The text itself
//! stays in the page for screen readers, search and copying.

use anyhow::{Context, Result, anyhow, bail};
use std::fmt::Write;
use ttf_parser::{Face, OutlineBuilder, Tag, name_id};

pub struct Kanji {
    data: Vec<u8>,
    index: u32,
    weight: f32,
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

impl Kanji {
    /// Opens the Japanese face of a Noto Serif CJK collection.
    pub fn load(path: &std::path::Path, weight: f32) -> Result<Self> {
        let data = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
        let count = ttf_parser::fonts_in_collection(&data).unwrap_or(1);
        for index in 0..count {
            let face = Face::parse(&data, index).map_err(|e| anyhow!("{e}"))?;
            let japanese = face.names().into_iter().any(|n| {
                n.name_id == name_id::FAMILY && n.to_string().is_some_and(|s| s.ends_with(" JP"))
            });
            if japanese {
                return Ok(Kanji {
                    data,
                    index,
                    weight,
                });
            }
        }
        bail!("{} has no Japanese face", path.display())
    }

    /// `text` drawn as an inline SVG one em tall, coloured by `currentColor`.
    pub fn svg(&self, text: &str) -> Result<String> {
        let mut face = Face::parse(&self.data, self.index).map_err(|e| anyhow!("{e}"))?;
        if face.is_variable() {
            face.set_variation(Tag::from_bytes(b"wght"), self.weight);
        }
        // The ideographic em box (typographic metrics), not the taller line
        // box, so a kanji is as tall as the text beside it.
        let top = face.typographic_ascender().unwrap_or(face.ascender()) as f32;
        let height = top - face.typographic_descender().unwrap_or(face.descender()) as f32;
        let mut pen = Pen {
            d: String::new(),
            dx: 0.0,
            top,
        };
        for ch in text.chars() {
            let gid = face
                .glyph_index(ch)
                .ok_or_else(|| anyhow!("Noto Serif CJK JP has no {ch}"))?;
            face.outline_glyph(gid, &mut pen);
            pen.dx += face.glyph_hor_advance(gid).unwrap_or(1000) as f32;
        }
        Ok(format!(
            r#"<svg class="kanji" viewBox="0 0 {:.0} {height:.0}" aria-hidden="true" focusable="false"><path d="{}"/></svg>"#,
            pen.dx, pen.d
        ))
    }
}
