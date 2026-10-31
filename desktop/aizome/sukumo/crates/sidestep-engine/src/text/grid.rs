//! Terminal-grid shaping, independent of the drawing backend.
//!
//! Font discovery and shaping belong on a worker. Immutable results contain
//! registered face identities and owned glyph positions; raster threads never
//! borrow the shaper. Cell ownership comes from the terminal, not Unicode width
//! guesses or paragraph bidi. Cluster placement adapts Ghostty's MIT-licensed
//! `src/font/shaper/harfbuzz.zig` at 6ba144158577d033ccb6ed4b574e4013957c9161.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use harfrust::{
    BufferClusterLevel, Direction, Feature, FontRef, ShapeOptions, ShaperData, ShaperInstance, UnicodeBuffer,
};
use icu_properties::{CodePointSetData, props::EmojiPresentation};
use parley::fontique::{
    Attributes, FontStyle, FontWeight, FontWidth, GenericFamily, QueryFamily, QueryFont, QueryStatus, Script,
};
use parley::{FontContext, FontData};
use skrifa::{MetadataProvider, instance::Size, raw::TableProvider};

use super::fonts::{self, Synth};
use crate::protocol::Glyph;

const CACHE_BYTES: usize = 8 * 1024 * 1024;
const CACHE_ENTRIES: usize = 512;
const MAX_FACES: usize = 128;
const MAX_RESOLUTIONS: usize = 2048;
const MAX_RUN_BYTES: usize = 64 * 1024;
const MAX_CELLS: usize = 8192;

#[derive(Clone, Debug, PartialEq)]
pub struct FontConfig {
    pub families: Vec<String>,
    pub fallback_families: Vec<String>,
    pub emoji_families: Vec<String>,
    /// Typographic points: 1pt = 96/72 logical units, before output scale.
    pub size: f32,
    pub weight: u16,
    /// HarfBuzz feature strings, such as `liga=0` or `cv01=2`.
    pub features: Vec<String>,
}

impl Default for FontConfig {
    fn default() -> Self {
        Self {
            families: vec!["IBM Plex Mono".into()],
            fallback_families: vec!["Noto Sans Mono CJK JP".into(), "Noto Sans Symbols 2".into()],
            emoji_families: vec!["Noto Color Emoji".into()],
            size: 14.0,
            weight: 400,
            features: Vec::new(),
        }
    }
}

impl FontConfig {
    /// Validate without loading fonts or touching the filesystem.
    pub fn validate(&self) -> Result<(), Error> {
        if !self.size.is_finite()
            || !(1.0..=256.0).contains(&self.size)
            || !(100..=900).contains(&self.weight)
            || self.features.len() > 64
            || [&self.families, &self.fallback_families, &self.emoji_families].iter().any(|list| {
                list.len() > 32 || list.iter().any(|s| s.is_empty() || s.len() > 256 || s.chars().any(char::is_control))
            })
            || self.features.iter().any(|s| s.len() > 64 || s.parse::<Feature>().is_err())
        {
            return Err(Error::InvalidConfig);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Style {
    pub bold: bool,
    pub italic: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct Cell<'a> {
    pub text: &'a str,
    pub column: u32,
    pub width: u8,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Metrics {
    pub scale: f64,
    pub cell_width_px: u32,
    pub cell_height_px: u32,
    pub baseline_px: f32,
    pub underline_y_px: f32,
    pub underline_thickness_px: f32,
    /// Ghostty Linux sprite policy; independent of native decoration metrics.
    pub box_thickness_px: u32,
    pub box_thickness_estimated: bool,
    pub strike_y_px: f32,
    pub strike_thickness_px: f32,
}

impl Metrics {
    pub fn cell_width(self) -> f64 {
        f64::from(self.cell_width_px) / self.scale
    }
    pub fn cell_height(self) -> f64 {
        f64::from(self.cell_height_px) / self.scale
    }
    pub fn baseline(self) -> f64 {
        f64::from(self.baseline_px) / self.scale
    }
}

/// A renderer-independent run. Glyph coordinates include cell positions and
/// are logical units from the row's left edge and common baseline (y down).
#[derive(Clone, Debug)]
pub struct Run {
    pub font: u32,
    pub family: Arc<str>,
    pub size: f32,
    pub glyphs: Arc<[Glyph]>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Cluster {
    pub column: u32,
    pub width: u32,
    pub font: u32,
}

#[derive(Clone, Debug)]
pub struct Layout {
    pub metrics: Metrics,
    pub runs: Arc<[Run]>,
    pub clusters: Arc<[Cluster]>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    InvalidConfig,
    NoFont,
    LimitExceeded,
    Cancelled,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::InvalidConfig => "invalid terminal font configuration",
            Self::NoFont => "no usable terminal font found",
            Self::LimitExceeded => "terminal shaping resource limit exceeded",
            Self::Cancelled => "terminal shaping cancelled",
        })
    }
}
impl std::error::Error for Error {}

struct Face {
    id: u32,
    family: Arc<str>,
    data: FontData,
    shaper: ShaperData,
    instance: ShaperInstance,
    color: bool,
    // Normalized to one em, for primary grid metrics and fallback fitting.
    ascent: f32,
    descent: f32,
    leading: f32,
    advance: f32,
    underline: (f32, f32),
    box_thickness_em: f64,
    box_thickness_estimated: bool,
    strike: (f32, f32),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct Key {
    cells: Vec<(String, u32, u8)>,
    style: Style,
    scale: u64,
    cursor: Option<u32>,
}

/// Worker-owned shaper. No mutex is held by callers while discovering or shaping.
pub struct FontSet {
    config: FontConfig,
    context: FontContext,
    faces: Vec<Face>,
    primary: usize,
    features: Vec<Feature>,
    resolutions: HashMap<(String, Style), usize>,
    resolution_bytes: usize,
    cache: HashMap<Key, Arc<Layout>>,
    cache_bytes: usize,
    buffer: Option<UnicodeBuffer>,
    pub missing_families: Vec<String>,
}

impl FontSet {
    pub fn new(config: FontConfig) -> Result<Self, Error> {
        config.validate()?;
        let mut features = vec!["liga=1".parse().map_err(|_| Error::InvalidConfig)?];
        for feature in &config.features {
            if feature.len() > 64 {
                return Err(Error::InvalidConfig);
            }
            features.push(feature.parse().map_err(|_| Error::InvalidConfig)?);
        }
        let context = fonts::shared().font_context();
        let mut set = Self {
            config,
            context,
            faces: Vec::new(),
            primary: 0,
            features,
            resolutions: HashMap::new(),
            resolution_bytes: 0,
            cache: HashMap::new(),
            cache_bytes: 0,
            buffer: Some(UnicodeBuffer::new()),
            missing_families: Vec::new(),
        };
        for family in set.config.families.iter().chain(&set.config.fallback_families).chain(&set.config.emoji_families)
        {
            if set.context.collection.family_id(family).is_none() {
                set.missing_families.push(family.clone());
            }
        }
        let names = set.config.families.clone();
        let candidate = set.find(&names, Style::default(), "M", None, true).ok_or(Error::NoFont)?;
        set.primary = set.add_face(candidate)?;
        Ok(set)
    }

    pub fn primary_family(&self) -> &str {
        &self.faces[self.primary].family
    }
    pub fn config(&self) -> &FontConfig {
        &self.config
    }

    pub fn metrics(&self, scale: f64) -> Result<Metrics, Error> {
        if !scale.is_finite() || !(0.25..=8.0).contains(&scale) {
            return Err(Error::InvalidConfig);
        }
        let face = &self.faces[self.primary];
        let em = self.config.size * (96.0 / 72.0) * scale as f32;
        let height = (face.ascent - face.descent + face.leading) * em;
        let cell_height_px = height.round().max(1.0) as u32;
        let baseline_px = ((face.ascent + face.leading / 2.0) * em + (cell_height_px as f32 - height) / 2.0).round();
        Ok(Metrics {
            scale,
            cell_width_px: (face.advance * em).round().max(1.0) as u32,
            cell_height_px,
            baseline_px,
            underline_y_px: (baseline_px - face.underline.0 * em).round(),
            underline_thickness_px: (face.underline.1 * em).ceil().max(1.0),
            box_thickness_px: (face.box_thickness_em * linux_ppem(self.config.size, scale)).ceil().max(1.0) as u32,
            box_thickness_estimated: face.box_thickness_estimated,
            strike_y_px: (baseline_px - face.strike.0 * em).round(),
            strike_thickness_px: (face.strike.1 * em).ceil().max(1.0),
        })
    }

    fn find(
        &mut self,
        names: &[String],
        style: Style,
        text: &str,
        presentation: Option<bool>,
        system: bool,
    ) -> Option<QueryFont> {
        let weight = if style.bold { self.config.weight.max(700) } else { self.config.weight };
        let mut guess = UnicodeBuffer::new();
        guess.push_str(text);
        guess.guess_segment_properties();
        let script = Script::from_bytes(guess.script().tag().into_bytes());
        let collection = &mut self.context.collection;
        let mut query = collection.query(&mut self.context.source_cache);
        let mut families: Vec<_> = names.iter().map(|n| QueryFamily::Named(n.as_str())).collect();
        if system {
            families.extend([
                QueryFamily::Generic(GenericFamily::Monospace),
                QueryFamily::Generic(GenericFamily::SansSerif),
            ]);
        }
        query.set_families(families);
        query.set_attributes(Attributes::new(
            FontWidth::NORMAL,
            if style.italic { FontStyle::Italic } else { FontStyle::Normal },
            FontWeight::new(f32::from(weight)),
        ));
        if system {
            query.set_fallbacks(script);
        }
        let mut found = None;
        let mut examined = 0;
        query.matches_with(|candidate| {
            examined += 1;
            if examined > MAX_FACES {
                return QueryStatus::Stop;
            }
            let Some(charmap) = candidate.charmap() else { return QueryStatus::Continue };
            if !text.chars().filter(|c| !ignorable(*c)).all(|c| charmap.map(c).is_some_and(|g| g != 0)) {
                return QueryStatus::Continue;
            }
            // Inspect candidates without retaining rejected faces in the
            // process-wide renderer registry.
            let color = FontRef::from_index(candidate.blob.data(), candidate.index).is_ok_and(|font| {
                use skrifa::raw::types::Tag;
                [*b"COLR", *b"CBDT", *b"sbix", *b"SVG "].iter().any(|tag| font.table_data(Tag::new(tag)).is_some())
            });
            if presentation.is_none_or(|wanted| color == wanted) {
                found = Some(candidate.clone());
                return QueryStatus::Stop;
            }
            if examined >= MAX_FACES { QueryStatus::Stop } else { QueryStatus::Continue }
        });
        found
    }

    fn add_face(&mut self, candidate: QueryFont) -> Result<usize, Error> {
        let data = FontData::new(candidate.blob.clone(), candidate.index);
        let font = FontRef::from_index(data.data.data(), data.index).map_err(|_| Error::NoFont)?;
        let instance = ShaperInstance::from_variations(
            &font,
            candidate.synthesis.variation_settings().iter().map(|&(tag, value)| (tag, value)),
        );
        let coords: Vec<i16> = instance.coords().iter().map(|v| v.to_bits()).collect();
        let synth = Synth {
            embolden: candidate.synthesis.embolden(),
            skew: candidate.synthesis.skew().unwrap_or(0.0),
            stroke: 0.0,
        };
        let id = fonts::register(&data, &coords, synth);
        if let Some(at) = self.faces.iter().position(|f| f.id == id) {
            return Ok(at);
        }
        if self.faces.len() >= MAX_FACES {
            return Err(Error::LimitExceeded);
        }
        let metrics = font.metrics(Size::unscaled(), instance.coords());
        let units = f32::from(metrics.units_per_em.max(1));
        let widths = font.glyph_metrics(Size::unscaled(), instance.coords());
        let charmap = font.charmap();
        let advance = (' '..='~')
            .filter_map(|c| charmap.map(c).and_then(|g| widths.advance_width(g)))
            .fold(0.0f32, f32::max)
            .max(1.0)
            / units;
        // Pinned Ghostty FreeType face reads the raw post thickness, then uses
        // Metrics.underlineThickness: positive post, else15% of ex-height.
        // Old fonts without OS/2 heights use unhinted native glyph bounds here;
        // FreeType-loaded/hinted fallback measurement parity is not claimed.
        let post_thickness = font.post().ok().map(|p| f64::from(p.underline_thickness().to_i16())).filter(|t| *t > 0.0);
        let height = |character| {
            charmap
                .map(character)
                .and_then(|g| widths.bounds(g))
                .map(|b| f64::from(b.y_max - b.y_min))
                .filter(|h| *h > 0.0)
        };
        let os2 = font.os2().ok();
        let table_height = |cap: bool| {
            os2.as_ref()
                .and_then(|o| {
                    if o.version() < 2 {
                        return None;
                    }
                    if cap { o.s_cap_height() } else { o.sx_height() }
                })
                .map(f64::from)
                .filter(|h| *h > 0.0)
        };
        let old_table = os2.as_ref().is_none_or(|o| o.version() < 2);
        let cap = table_height(true)
            .or_else(|| old_table.then(|| height('H')).flatten())
            .unwrap_or(0.75 * f64::from(metrics.ascent));
        let ex = table_height(false).or_else(|| old_table.then(|| height('x')).flatten()).unwrap_or(0.75 * cap);
        let box_thickness_em = post_thickness.unwrap_or(0.15 * ex) / f64::from(units);
        let family = self.context.collection.family_name(candidate.family.0).unwrap_or("Unknown").into();
        let shaper = ShaperData::new(&font);
        let face = Face {
            id,
            family,
            color: fonts::has_color(id),
            data,
            shaper,
            instance,
            ascent: metrics.ascent / units,
            descent: metrics.descent / units,
            leading: metrics.leading / units,
            advance,
            box_thickness_em,
            box_thickness_estimated: post_thickness.is_none(),
            underline: metrics.underline.map_or((-0.1, 0.05), |d| (d.offset / units, d.thickness / units)),
            strike: metrics.strikeout.map_or((0.3, 0.05), |d| (d.offset / units, d.thickness / units)),
        };
        self.faces.push(face);
        Ok(self.faces.len() - 1)
    }

    fn resolve(&mut self, text: &str, style: Style) -> Result<usize, Error> {
        if let Some(&face) = self.resolutions.get(&(text.to_owned(), style)) {
            return Ok(face);
        }
        let explicit = if text.contains('\u{fe0e}') {
            Some(false)
        } else if text.contains('\u{fe0f}') {
            Some(true)
        } else {
            None
        };
        let emoji = explicit.unwrap_or_else(|| {
            text.chars().next().is_some_and(|c| CodePointSetData::new::<EmojiPresentation>().contains(c))
        });
        let mut names = self.config.families.clone();
        // A missing requested family resolved to a concrete system primary.
        // Its regular/style faces still precede unrelated fallback families.
        let primary = self.faces[self.primary].family.to_string();
        if !names.contains(&primary) {
            names.push(primary);
        }
        let mut candidate = self.find(&names, style, text, explicit, false);
        if candidate.is_none() {
            let mut names =
                if emoji { self.config.emoji_families.clone() } else { self.config.fallback_families.clone() };
            if emoji {
                names.extend(self.config.fallback_families.iter().cloned());
            }
            candidate = self.find(&names, style, text, Some(emoji), true);
        }
        // Best effort last resort preserves the original text and column even
        // when no installed face can satisfy all combining/presentation glyphs.
        let face = match candidate {
            Some(c) => self.add_face(c)?,
            None => self.primary,
        };
        if self.resolutions.len() >= MAX_RESOLUTIONS || self.resolution_bytes.saturating_add(text.len()) > 1024 * 1024 {
            self.resolutions.clear();
            self.resolution_bytes = 0;
        }
        self.resolution_bytes += text.len();
        self.resolutions.insert((text.to_owned(), style), face);
        Ok(face)
    }

    pub fn shape(
        &mut self,
        cells: &[Cell<'_>],
        style: Style,
        scale: f64,
        cursor: Option<u32>,
        cancel: &AtomicBool,
    ) -> Result<Arc<Layout>, Error> {
        if cancel.load(Ordering::Relaxed) {
            return Err(Error::Cancelled);
        }
        let bytes = cells.iter().try_fold(0usize, |n, c| n.checked_add(c.text.len())).ok_or(Error::LimitExceeded)?;
        if cells.len() > MAX_CELLS
            || bytes > MAX_RUN_BYTES
            || cells.iter().any(|c| c.width == 0 || c.width > 2 || c.column > 65535)
        {
            return Err(Error::LimitExceeded);
        }
        let metrics = self.metrics(scale)?;
        let key = Key {
            cells: cells.iter().map(|c| (c.text.to_owned(), c.column, c.width)).collect(),
            style,
            scale: scale.to_bits(),
            cursor,
        };
        if let Some(layout) = self.cache.get(&key) {
            return Ok(layout.clone());
        }
        let mut runs = Vec::new();
        let mut clusters = Vec::new();
        let mut start = 0;
        while start < cells.len() {
            if cancel.load(Ordering::Relaxed) {
                return Err(Error::Cancelled);
            }
            let face = self.resolve(cells[start].text, style)?;
            let mut end = start + 1;
            while end < cells.len() {
                let previous = &cells[end - 1];
                let next = &cells[end];
                if next.column != previous.column + u32::from(previous.width)
                    || cursor.is_some_and(|c| c == next.column || c == previous.column)
                    || self.resolve(next.text, style)? != face
                {
                    break;
                }
                end += 1;
            }
            let (shaped, spans) = self.shape_run(face, &cells[start..end], metrics)?;
            runs.extend(shaped);
            clusters.extend(spans);
            start = end;
        }
        let output = Arc::new(Layout { metrics, runs: runs.into(), clusters: clusters.into() });
        let cost = bytes
            + cells.len() * std::mem::size_of::<(String, u32, u8)>()
            + output.runs.iter().map(|r| r.glyphs.len() * std::mem::size_of::<Glyph>()).sum::<usize>()
            + output.clusters.len() * std::mem::size_of::<Cluster>();
        if self.cache.len() >= CACHE_ENTRIES || self.cache_bytes.saturating_add(cost) > CACHE_BYTES {
            self.cache.clear();
            self.cache_bytes = 0;
        }
        if cost <= CACHE_BYTES {
            self.cache.insert(key, output.clone());
            self.cache_bytes += cost;
        }
        Ok(output)
    }

    fn shape_run(
        &mut self,
        face_index: usize,
        cells: &[Cell<'_>],
        metrics: Metrics,
    ) -> Result<(Vec<Run>, Vec<Cluster>), Error> {
        let mut buffer = self.buffer.take().unwrap_or_default();
        let mut inputs = Vec::new();
        for cell in cells {
            for ch in cell.text.chars() {
                buffer.add(ch, inputs.len() as u32);
                inputs.push((cell.column, u32::from(cell.width)));
            }
        }
        buffer.set_direction(Direction::LeftToRight);
        buffer.set_cluster_level(BufferClusterLevel::Characters);
        buffer.guess_segment_properties();
        let face = &self.faces[face_index];
        let logical_em = self.config.size * (96.0 / 72.0);
        let pixel_em = logical_em * metrics.scale as f32;
        let font = FontRef::from_index(face.data.data.data(), face.data.index).map_err(|_| Error::NoFont)?;
        let shaper = face.shaper.shaper(&font).instance(Some(&face.instance)).build();
        let shaped = shaper.shape(
            buffer,
            ShapeOptions::new()
                .scale(Some((pixel_em * 64.0).round() as i32))
                .point_size(Some(self.config.size))
                .features(&self.features),
        );
        let mut glyphs = Vec::with_capacity(shaped.len());
        let mut clusters = Vec::new();
        let mut cluster_indices: Vec<_> = shaped.glyph_infos().iter().map(|info| info.cluster as usize).collect();
        cluster_indices.sort_unstable();
        cluster_indices.dedup();
        let extent = |index: usize| {
            let next = cluster_indices.partition_point(|&other| other <= index);
            let end = cluster_indices.get(next).copied().unwrap_or(inputs.len()).min(inputs.len());
            let slice = &inputs[index.min(end)..end];
            let column = slice.iter().map(|&(column, _)| column).min().unwrap_or(0);
            let right = slice.iter().map(|&(column, width)| column + width).max().unwrap_or(column);
            (column, right - column)
        };
        let mut pen_x = 0i64;
        let mut pen_y = 0i64;
        let mut anchor_pen = 0i64;
        let mut anchor_column = cells.first().map_or(0, |c| c.column);
        let mut last_column = None;
        let mut max_column = 0;
        for (info, position) in shaped.glyph_infos().iter().zip(shaped.glyph_positions()) {
            let index = info.cluster as usize;
            let Some(&(column, _)) = inputs.get(index) else { continue };
            if last_column != Some(column) {
                let first = index == 0 || inputs[index - 1].0 != column;
                if first && (last_column.is_none() || column > max_column) {
                    anchor_pen = pen_x;
                    anchor_column = column;
                }
                let (column, width) = extent(index);
                clusters.push(Cluster { column, width, font: face.id });
                last_column = Some(column);
            }
            max_column = max_column.max(column);
            glyphs.push(Glyph {
                id: info.glyph_id,
                x: anchor_column as f32 * metrics.cell_width() as f32
                    + ((pen_x - anchor_pen + i64::from(position.x_offset)) as f32 / 64.0) / metrics.scale as f32,
                y: -((pen_y + i64::from(position.y_offset)) as f32 / 64.0) / metrics.scale as f32,
            });
            pen_x += i64::from(position.x_advance);
            pen_y += i64::from(position.y_advance);
        }
        let mut runs = Vec::new();
        if face.color {
            // Font metrics define the bitmap em box even for CBDT fonts whose
            // outline bounds are absent. Shape across adjacent legacy cells
            // first, so modifiers/ZWJ/flags join; then fit each output cluster
            // inside its existing reservation without moving the next cell.
            let natural_height = ((face.ascent - face.descent) * logical_em).max(1.0);
            let mut start = 0;
            while start < glyphs.len() {
                let index = shaped.glyph_infos()[start].cluster;
                let mut end = start + 1;
                while end < glyphs.len() && shaped.glyph_infos()[end].cluster == index {
                    end += 1;
                }
                let (column, width) = extent(index as usize);
                let reserved = width as f32 * metrics.cell_width() as f32;
                let advance: i64 = shaped.glyph_positions()[start..end].iter().map(|p| i64::from(p.x_advance)).sum();
                let natural_width = (advance as f32 / 64.0 / metrics.scale as f32).abs().max(1.0);
                let factor =
                    (reserved * 0.95 / natural_width).min(metrics.cell_height() as f32 / natural_height).min(1.0);
                let left = column as f32 * metrics.cell_width() as f32;
                let center = (reserved - natural_width * factor) / 2.0;
                let baseline_shift = (metrics.cell_height() as f32 - natural_height * factor) / 2.0
                    + face.ascent * logical_em * factor
                    - metrics.baseline() as f32;
                for glyph in &mut glyphs[start..end] {
                    glyph.x = left + (glyph.x - left) * factor + center;
                    glyph.y = glyph.y * factor + baseline_shift;
                }
                runs.push(Run {
                    font: face.id,
                    family: face.family.clone(),
                    size: logical_em * factor,
                    glyphs: glyphs[start..end].into(),
                });
                start = end;
            }
        } else {
            runs.push(Run { font: face.id, family: face.family.clone(), size: logical_em, glyphs: glyphs.into() });
        }
        self.buffer = Some(shaped.clear());
        Ok((runs, clusters))
    }
}

fn ignorable(ch: char) -> bool {
    matches!(ch, '\u{200c}' | '\u{200d}' | '\u{fe0e}' | '\u{fe0f}' | '\u{e0020}'..='\u{e007f}')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set() -> FontSet {
        FontSet::new(FontConfig::default()).expect("system monospace font")
    }
    fn cells<'a>(text: &[&'a str]) -> Vec<Cell<'a>> {
        text.iter().enumerate().map(|(column, text)| Cell { text, column: column as u32, width: 1 }).collect()
    }

    #[test]
    fn primary_grid_rounds_physical_metrics_and_keeps_columns() {
        let mut fonts = set();
        for scale in [1.0, 1.25, 2.0] {
            let metrics = fonts.metrics(scale).unwrap();
            assert_eq!(metrics.cell_width() * scale, f64::from(metrics.cell_width_px));
            assert_eq!(metrics.cell_height() * scale, f64::from(metrics.cell_height_px));
            let layout = fonts
                .shape(&cells(&["M", "i", "W", " ", "M"]), Style::default(), scale, None, &AtomicBool::new(false))
                .unwrap();
            let glyphs: Vec<_> = layout.runs.iter().flat_map(|run| run.glyphs.iter()).collect();
            assert_eq!(glyphs.len(), 5);
            for (column, glyph) in glyphs.iter().enumerate() {
                assert!((f64::from(glyph.x) - column as f64 * metrics.cell_width()).abs() < 0.001);
            }
            assert!(layout.runs.iter().all(|run| run.family.as_ref() == fonts.primary_family()));
        }
    }

    #[test]
    fn combining_cluster_does_not_move_following_cell() {
        let mut fonts = set();
        let layout =
            fonts.shape(&cells(&["a\u{301}", "M"]), Style::default(), 1.25, None, &AtomicBool::new(false)).unwrap();
        let last = layout.runs.last().unwrap().glyphs.last().unwrap();
        assert!((f64::from(last.x) - layout.metrics.cell_width()).abs() < 0.001);
        assert_eq!(layout.clusters.last().unwrap().column, 1);
    }

    #[test]
    fn cache_keys_compare_cells_and_cursor_and_reject_invalid_work() {
        let mut fonts = set();
        let cancel = AtomicBool::new(false);
        let a = fonts.shape(&cells(&["M", "M"]), Style::default(), 1.0, None, &cancel).unwrap();
        let b = fonts.shape(&cells(&["M", "M"]), Style::default(), 1.0, None, &cancel).unwrap();
        assert!(Arc::ptr_eq(&a, &b));
        let c = fonts.shape(&cells(&["M", "M"]), Style::default(), 1.0, Some(1), &cancel).unwrap();
        assert!(!Arc::ptr_eq(&a, &c));
        assert_eq!(c.runs.len(), 2);
        cancel.store(true, Ordering::Relaxed);
        assert!(matches!(fonts.shape(&cells(&["M"]), Style::default(), 1.0, None, &cancel), Err(Error::Cancelled)));
        cancel.store(false, Ordering::Relaxed);
        let huge = "M".repeat(MAX_RUN_BYTES + 1);
        assert!(matches!(
            fonts.shape(&cells(&[&huge]), Style::default(), 1.0, None, &cancel),
            Err(Error::LimitExceeded)
        ));
        assert!(fonts.metrics(f64::NAN).is_err());
    }

    #[test]
    fn invalid_configuration_never_loads_a_font() {
        let mut config = FontConfig::default();
        config.features.push("this-is-not-an-opentype-feature".into());
        assert!(config.validate().is_err());
        config.features = vec!["liga=0".into(), "cv01=2".into()];
        assert!(config.validate().is_ok());
        config.size = f32::INFINITY;
        assert!(config.validate().is_err());
    }

    fn install_packaged_fonts() {
        let directory = std::path::PathBuf::from(std::env::var_os("HOTARU_FONT_DIR").expect("HOTARU_FONT_DIR fixture"));
        for name in ["NotoColorEmoji.ttf", "NotoSansCJK-Regular.ttc"] {
            assert!(!fonts::install(std::fs::read(directory.join(name)).unwrap()).is_empty());
        }
        if let Some(directory) = std::env::var_os("AIZOME_FONT_DIR") {
            let directory = std::path::PathBuf::from(directory);
            for name in [
                "IBMPlexMono-Regular.ttf",
                "IBMPlexMono-Bold.ttf",
                "IBMPlexMono-Italic.ttf",
                "NotoSansSymbols2-Regular.otf",
            ] {
                assert!(!fonts::install(std::fs::read(directory.join(name)).unwrap()).is_empty());
            }
        }
    }

    #[test]
    #[ignore = "requires pinned HOTARU_FONT_DIR and AIZOME_FONT_DIR; run fixture suite explicitly"]
    fn packaged_cjk_emoji_presentation_and_following_columns() {
        install_packaged_fonts();
        let mut fonts = set();
        assert_eq!(fonts.primary_family(), "IBM Plex Mono");
        for scale in [1.0, 1.25, 2.0] {
            for (text, family) in [
                ("蛍", "Noto Sans Mono CJK JP"),
                ("😀", "Noto Color Emoji"),
                ("👍🏽", "Noto Color Emoji"),
                ("👩\u{200d}💻", "Noto Color Emoji"),
                ("🇯🇵", "Noto Color Emoji"),
                ("❤\u{fe0f}", "Noto Color Emoji"),
                ("1\u{fe0f}\u{20e3}", "Noto Color Emoji"),
            ] {
                let cells = [Cell { text, column: 0, width: 2 }, Cell { text: "M", column: 2, width: 1 }];
                let layout = fonts.shape(&cells, Style::default(), scale, None, &AtomicBool::new(false)).unwrap();
                assert_eq!(layout.runs[0].family.as_ref(), family, "{text:?}");
                assert!(layout.runs.iter().flat_map(|r| r.glyphs.iter()).all(|g| g.id != 0), "missing {text:?}");
                if family == "Noto Color Emoji" {
                    assert_eq!(layout.runs[0].glyphs.len(), 1, "sequence {text:?}");
                }
                assert!(
                    (f64::from(layout.runs.last().unwrap().glyphs.last().unwrap().x)
                        - 2.0 * layout.metrics.cell_width())
                    .abs()
                        < 0.001
                );
                assert_eq!(layout.metrics, fonts.metrics(scale).unwrap());
            }
            let text = fonts
                .shape(
                    &[Cell { text: "❤\u{fe0e}", column: 0, width: 1 }],
                    Style::default(),
                    scale,
                    None,
                    &AtomicBool::new(false),
                )
                .unwrap();
            assert!(!fonts::has_color(text.runs[0].font));
            assert!(text.runs[0].glyphs.iter().all(|g| g.id != 0));
        }
    }

    #[test]
    #[ignore = "requires pinned HOTARU_TEST_LIGATURE_FONT; run fixture suite explicitly"]
    fn ligature_features_cursor_boundaries_and_combining_fixture() {
        let path = std::env::var_os("HOTARU_TEST_LIGATURE_FONT").expect("pinned ligature font fixture");
        let names = fonts::install(std::fs::read(path).unwrap());
        assert!(!names.is_empty());
        let config = FontConfig { families: vec![names[0].to_string()], ..FontConfig::default() };
        let mut enabled = FontSet::new(config.clone()).unwrap();
        let mut disabled =
            FontSet::new(FontConfig { features: vec!["liga=0".into(), "calt=0".into()], ..config }).unwrap();
        let cancel = AtomicBool::new(false);
        let chars = cells(&["=", "=", "=", "M"]);
        let joined = enabled.shape(&chars, Style::default(), 1.0, None, &cancel).unwrap();
        let separate = disabled.shape(&chars, Style::default(), 1.0, None, &cancel).unwrap();
        let joined_ids: Vec<_> = joined.runs.iter().flat_map(|r| r.glyphs.iter().map(|g| g.id)).collect();
        let separate_ids: Vec<_> = separate.runs.iter().flat_map(|r| r.glyphs.iter().map(|g| g.id)).collect();
        assert_ne!(joined_ids, separate_ids, "fixture must actually exercise a programming ligature");
        let cursor = enabled.shape(&chars, Style::default(), 1.0, Some(1), &cancel).unwrap();
        assert!(cursor.runs.len() >= 3);
        let chars = cells(&["a\u{301}", "M"]);
        let with_cursor = enabled.shape(&chars, Style::default(), 1.0, Some(0), &cancel).unwrap();
        assert!(
            (f64::from(with_cursor.runs.last().unwrap().glyphs.last().unwrap().x) - with_cursor.metrics.cell_width())
                .abs()
                < 0.001
        );
    }

    #[test]
    #[ignore = "requires pinned HOTARU_FONT_DIR; run fixture suite explicitly"]
    fn legacy_color_cells_join_without_changing_their_reservation() {
        install_packaged_fonts();
        let mut fonts = set();
        for (first, second) in [("👍", "🏽"), ("👩\u{200d}", "💻"), ("🇯", "🇵")] {
            let cells = [
                Cell { text: first, column: 0, width: 2 },
                Cell { text: second, column: 2, width: 2 },
                Cell { text: "M", column: 4, width: 1 },
            ];
            let layout = fonts.shape(&cells, Style::default(), 1.25, None, &AtomicBool::new(false)).unwrap();
            assert_eq!(layout.runs[0].family.as_ref(), "Noto Color Emoji");
            assert_eq!(layout.runs[0].glyphs.len(), 1, "{first}{second}");
            assert_eq!(layout.clusters[0].width, 4);
            assert!(
                (f64::from(layout.runs.last().unwrap().glyphs[0].x) - 4.0 * layout.metrics.cell_width()).abs() < 0.001
            );
            let cursor = fonts.shape(&cells, Style::default(), 1.25, Some(2), &AtomicBool::new(false)).unwrap();
            assert!(cursor.runs.len() >= 3);
        }
    }
}

/// Matches Linux FT_Set_Char_Size rounding used by the pinned Ghostty face:
/// round points to26.6, round DPI, round pixel26.6, then integer y_ppem.
fn linux_ppem(points: f32, scale: f64) -> f64 {
    let points26 = (f64::from(points) * 64.0).round().max(1.0) as u64;
    let dpi = (96.0 * scale).round().max(1.0) as u64;
    let pixels26 = (points26 * dpi + 36) / 72;
    ((pixels26 + 32) / 64).max(1) as f64
}

#[cfg(test)]
#[test]
fn linux_sprite_ppem_matches_freetype_126_recorded_sizes() {
    // FT_Set_Char_Size oracle; IBM Plex Mono font SHA256
    // 7c6fbddca4b700be918f5f6183d9bd4464fa427fe435f0b480d77fe2bb8c5a43.
    // Raw oracle/FreeType helper provenance lives in Hotaru sprite evidence.
    let cases: &[(f32, f64, u32)] = &[
        (6.0, 0.25, 2),
        (6.0, 0.5, 4),
        (6.0, 1.0, 8),
        (6.0, 1.25, 10),
        (6.0, 1.5, 12),
        (6.0, 1.75, 14),
        (6.0, 2.0, 16),
        (6.0, 3.0, 24),
        (6.0, 8.0, 64),
        (8.0, 0.25, 3),
        (8.0, 0.5, 5),
        (8.0, 1.0, 11),
        (8.0, 1.25, 13),
        (8.0, 1.5, 16),
        (8.0, 1.75, 19),
        (8.0, 2.0, 21),
        (8.0, 3.0, 32),
        (8.0, 8.0, 85),
        (10.0, 0.25, 3),
        (10.0, 0.5, 7),
        (10.0, 1.0, 13),
        (10.0, 1.25, 17),
        (10.0, 1.5, 20),
        (10.0, 1.75, 23),
        (10.0, 2.0, 27),
        (10.0, 3.0, 40),
        (10.0, 8.0, 107),
        (12.0, 0.25, 4),
        (12.0, 0.5, 8),
        (12.0, 1.0, 16),
        (12.0, 1.25, 20),
        (12.0, 1.5, 24),
        (12.0, 1.75, 28),
        (12.0, 2.0, 32),
        (12.0, 3.0, 48),
        (12.0, 8.0, 128),
        (13.49, 0.25, 5),
        (13.49, 0.5, 9),
        (13.49, 1.0, 18),
        (13.49, 1.25, 22),
        (13.49, 1.5, 27),
        (13.49, 1.75, 31),
        (13.49, 2.0, 36),
        (13.49, 3.0, 54),
        (13.49, 8.0, 144),
        (13.5, 0.25, 5),
        (13.5, 0.5, 9),
        (13.5, 1.0, 18),
        (13.5, 1.25, 23),
        (13.5, 1.5, 27),
        (13.5, 1.75, 32),
        (13.5, 2.0, 36),
        (13.5, 3.0, 54),
        (13.5, 8.0, 144),
        (13.51, 0.25, 5),
        (13.51, 0.5, 9),
        (13.51, 1.0, 18),
        (13.51, 1.25, 23),
        (13.51, 1.5, 27),
        (13.51, 1.75, 32),
        (13.51, 2.0, 36),
        (13.51, 3.0, 54),
        (13.51, 8.0, 144),
        (14.0, 0.25, 5),
        (14.0, 0.5, 9),
        (14.0, 1.0, 19),
        (14.0, 1.25, 23),
        (14.0, 1.5, 28),
        (14.0, 1.75, 33),
        (14.0, 2.0, 37),
        (14.0, 3.0, 56),
        (14.0, 8.0, 149),
        (16.0, 0.25, 5),
        (16.0, 0.5, 11),
        (16.0, 1.0, 21),
        (16.0, 1.25, 27),
        (16.0, 1.5, 32),
        (16.0, 1.75, 37),
        (16.0, 2.0, 43),
        (16.0, 3.0, 64),
        (16.0, 8.0, 171),
        (24.0, 0.25, 8),
        (24.0, 0.5, 16),
        (24.0, 1.0, 32),
        (24.0, 1.25, 40),
        (24.0, 1.5, 48),
        (24.0, 1.75, 56),
        (24.0, 2.0, 64),
        (24.0, 3.0, 96),
        (24.0, 8.0, 256),
        (32.0, 0.25, 11),
        (32.0, 0.5, 21),
        (32.0, 1.0, 43),
        (32.0, 1.25, 53),
        (32.0, 1.5, 64),
        (32.0, 1.75, 75),
        (32.0, 2.0, 85),
        (32.0, 3.0, 128),
        (32.0, 8.0, 341),
        (48.0, 0.25, 16),
        (48.0, 0.5, 32),
        (48.0, 1.0, 64),
        (48.0, 1.25, 80),
        (48.0, 1.5, 96),
        (48.0, 1.75, 112),
        (48.0, 2.0, 128),
        (48.0, 3.0, 192),
        (48.0, 8.0, 512),
        (64.0, 0.25, 21),
        (64.0, 0.5, 43),
        (64.0, 1.0, 85),
        (64.0, 1.25, 107),
        (64.0, 1.5, 128),
        (64.0, 1.75, 149),
        (64.0, 2.0, 171),
        (64.0, 3.0, 256),
        (64.0, 8.0, 683),
        (96.0, 0.25, 32),
        (96.0, 0.5, 64),
        (96.0, 1.0, 128),
        (96.0, 1.25, 160),
        (96.0, 1.5, 192),
        (96.0, 1.75, 224),
        (96.0, 2.0, 256),
        (96.0, 3.0, 384),
        (96.0, 8.0, 1024),
    ];
    for &(points, scale, ppem) in cases {
        assert_eq!(linux_ppem(points, scale), f64::from(ppem), "points{points} scale{scale}");
    }
}
