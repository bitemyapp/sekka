//! Text the compositor draws (tab titles, overview captions, switcher and
//! marking-menu labels), set by Sidestep's offscreen canvas so it matches
//! the apps' text pixel for pixel.
//!
//! Each label is rasterized once per text, style, scale and width and kept
//! as a [`MemoryRenderBuffer`] (uploaded to the GPU once per renderer);
//! labels nobody drew for a while are dropped. Nothing is rasterized per
//! frame.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use aizome_theme::{Rgba, typeface};
use sidestep_ui::kurbo::{Point, Size};
use sidestep_ui::{Appearance, Color, Font, TextLayout, TextStyle, Wrap};
use smithay::backend::allocator::Fourcc;
use smithay::backend::renderer::element::memory::MemoryRenderBuffer;
use smithay::utils::Transform;

/// The typefaces the compositor uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Face {
    /// Tab titles: Archivo 650 at 87.5% width.
    Tab,
    /// Background tabs: Archivo 500 at 87.5%.
    TabLight,
    /// Interface text, semibold.
    UiSemibold,
    /// Headings: Archivo 750.
    UiBold,
    /// IBM Plex Mono 400 (captions, shortcuts).
    Mono,
    /// IBM Plex Mono 600.
    MonoBold,
}

/// A face, a size (in quarter points) and a colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Style {
    pub face: Face,
    size_q: u16,
    pub color: Rgba,
}

impl Style {
    pub fn new(face: Face, size: f64, color: Rgba) -> Style {
        Style { face, size_q: (size * 4.0).round() as u16, color }
    }
    pub fn size(&self) -> f64 {
        self.size_q as f64 / 4.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct Key {
    text: String,
    style: Style,
    scale_q: u32,
    max_w: i32,
}

/// A rasterized label: its pixels and its size in points (whole points,
/// so it maps pixel to pixel at integer scales).
pub struct Label {
    pub buffer: MemoryRenderBuffer,
    pub w: i32,
    pub h: i32,
    last_used: Instant,
}

pub struct Labels {
    cache: HashMap<Key, Label>,
    widths: HashMap<(String, Face, u16), (f64, f64)>,
    fonts: HashMap<(Face, u16), Font>,
    last_gc: Instant,
}

impl Default for Labels {
    fn default() -> Self {
        Labels::new()
    }
}

impl Labels {
    pub fn new() -> Labels {
        install_fonts();
        Labels { cache: HashMap::new(), widths: HashMap::new(), fonts: HashMap::new(), last_gc: Instant::now() }
    }

    /// Loads the fonts and their first glyphs now, at start-up, rather
    /// than in the first frame that shows a label.
    pub fn warm_up(&mut self) {
        let started = Instant::now();
        for face in [Face::Tab, Face::TabLight, Face::UiSemibold, Face::UiBold, Face::Mono, Face::MonoBold] {
            let style = Style::new(face, typeface::TAB, Rgba::hex(0xffffff));
            let _ = self.get("Aizome 藍染 0123456789", style, 2.0, 400.0);
        }
        self.cache.clear();
        tracing::debug!("fonts warmed up in {:?}", started.elapsed());
    }

    fn font(&mut self, face: Face, size_q: u16) -> Font {
        self.fonts
            .entry((face, size_q))
            .or_insert_with(|| {
                let size = size_q as f64 / 4.0;
                let (family, weight) = match face {
                    Face::Tab => (typeface::UI, typeface::WEIGHT_TAB),
                    Face::TabLight => (typeface::UI, typeface::WEIGHT_MEDIUM),
                    Face::UiSemibold => (typeface::UI, typeface::WEIGHT_SEMIBOLD),
                    Face::UiBold => (typeface::UI, typeface::WEIGHT_BOLD),
                    Face::Mono => (typeface::MONO, typeface::WEIGHT_REGULAR),
                    Face::MonoBold => (typeface::MONO, typeface::WEIGHT_SEMIBOLD),
                };
                let base = Font::named(family, size).unwrap_or_else(|| match face {
                    Face::Mono | Face::MonoBold => Font::monospace(size),
                    _ => Font::system(size),
                });
                let font = base.with_weight(weight);
                // Tabs set Archivo semi-condensed (87.5%).
                match face {
                    Face::Tab | Face::TabLight => font.with_width(typeface::TAB_WIDTH),
                    _ => font,
                }
            })
            .clone()
    }

    fn text_style(&mut self, style: Style) -> TextStyle {
        let font = self.font(style.face, style.size_q);
        let c = style.color;
        TextStyle::new(font, Color::rgba8(c.r, c.g, c.b, c.a))
    }

    /// A text's width and height in points.
    pub fn measure(&mut self, text: &str, face: Face, size: f64) -> (f64, f64) {
        let size_q = (size * 4.0).round() as u16;
        let key = (text.to_string(), face, size_q);
        if let Some(m) = self.widths.get(&key) {
            return *m;
        }
        let style = self.text_style(Style { face, size_q, color: Rgba::hex(0) });
        let s = sidestep_ui::measure(text, &style);
        let m = (s.width, s.height);
        if self.widths.len() > 4096 {
            self.widths.clear();
        }
        self.widths.insert(key, m);
        m
    }

    /// The text shortened with an ellipsis to fit `max_w` points: in the
    /// middle for paths (so both ends show), at the end otherwise.
    pub fn fit(&mut self, text: &str, face: Face, size: f64, max_w: f64) -> String {
        if self.measure(text, face, size).0 <= max_w {
            return text.to_string();
        }
        let chars: Vec<char> = text.chars().collect();
        let middle = text.contains('/');
        let candidate = |keep: usize| -> String {
            if middle {
                let tail = keep / 2;
                let head = keep - tail;
                let mut s: String = chars[..head].iter().collect();
                s.push('…');
                s.extend(chars[chars.len() - tail..].iter());
                s
            } else {
                let mut s: String = chars[..keep].iter().collect::<String>().trim_end().to_string();
                s.push('…');
                s
            }
        };
        // The most characters that fit.
        let (mut lo, mut hi) = (0usize, chars.len());
        while lo < hi {
            let mid = (lo + hi).div_ceil(2);
            if self.measure(&candidate(mid), face, size).0 <= max_w {
                lo = mid;
            } else {
                hi = mid - 1;
            }
        }
        candidate(lo)
    }

    /// The label for a text, rasterized at `scale` and truncated with an
    /// ellipsis to `max_w` points.
    pub fn get(&mut self, text: &str, style: Style, scale: f64, max_w: f64) -> Option<&Label> {
        if text.is_empty() || max_w < 4.0 {
            return None;
        }
        // Integer buffer scales: fractional outputs downscale a sharper
        // label.
        let buffer_scale = scale.ceil().max(1.0);
        let key =
            Key { text: text.to_string(), style, scale_q: (buffer_scale * 4.0) as u32, max_w: max_w.floor() as i32 };
        if !self.cache.contains_key(&key) {
            let label = self.rasterize(text, style, buffer_scale, max_w.floor())?;
            self.cache.insert(key.clone(), label);
        }
        let label = self.cache.get_mut(&key)?;
        label.last_used = Instant::now();
        Some(label)
    }

    fn rasterize(&mut self, text: &str, style: Style, scale: f64, max_w: f64) -> Option<Label> {
        let ts = self.text_style(style);
        let (w, h) = self.measure(text, style.face, style.size());
        let truncate = w > max_w;
        let box_w = w.min(max_w).ceil().max(1.0);
        let box_h = h.ceil().max(1.0);
        let appearance = if style.color.luminance() > 0.5 { Appearance::DARK } else { Appearance::LIGHT };
        let image = sidestep_ui::offscreen::render(Size::new(box_w, box_h), scale, appearance, |c| {
            if truncate {
                let layout = TextLayout::builder(text, &ts).width(max_w).wrap(Wrap::Truncate).max_lines(1).build();
                c.draw_text(&layout, Point::ZERO);
            } else {
                c.draw_label(text, &ts, Point::ZERO);
            }
        });
        let (pw, ph) = image.pixel_size();
        let px = image.to_premultiplied_rgba8()?;
        let buffer = MemoryRenderBuffer::from_slice(
            &px,
            Fourcc::Abgr8888,
            (pw as i32, ph as i32),
            scale as i32,
            Transform::Normal,
            None,
        );
        Some(Label { buffer, w: box_w as i32, h: box_h as i32, last_used: Instant::now() })
    }

    /// Drops labels nobody drew for half a minute.
    pub fn gc(&mut self) {
        if self.last_gc.elapsed() < Duration::from_secs(10) {
            return;
        }
        self.last_gc = Instant::now();
        self.cache.retain(|_, l| l.last_used.elapsed() < Duration::from_secs(30));
    }

    /// Forgets everything (a theme or font change).
    pub fn clear(&mut self) {
        self.cache.clear();
    }

    pub fn len(&self) -> usize {
        self.cache.len()
    }

    pub fn is_empty(&self) -> bool {
        self.cache.is_empty()
    }
}

/// Installs Aizome's fonts for this process where fontconfig lacks them:
/// from `$AIZOME_FONT_DIR`, else `../share/aizome/fonts` beside the
/// executable (as the kit does), so tabs are set in Archivo and captions
/// in IBM Plex Mono even before the system has them.
fn install_fonts() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let mut dirs: Vec<std::path::PathBuf> = Vec::new();
        if let Some(d) = std::env::var_os("AIZOME_FONT_DIR") {
            dirs.push(d.into());
        }
        if let Some(bin) = std::env::current_exe().ok().and_then(|e| e.parent().map(|p| p.to_path_buf())) {
            dirs.push(bin.join("../share/aizome/fonts"));
        }
        let families: [(&str, &[&str]); 3] = [
            (typeface::UI, &["Archivo[wdth,wght]", "Archivo-VF"]),
            (
                typeface::MONO,
                &["IBMPlexMono-Regular", "IBMPlexMono-Medium", "IBMPlexMono-SemiBold", "IBMPlexMono-Bold"],
            ),
            (typeface::SYMBOLS, &["NotoSansSymbols2-Regular"]),
        ];
        for (family, stems) in families {
            if Font::named(family, 13.0).is_some_and(|f| f.family_name().eq_ignore_ascii_case(family)) {
                continue;
            }
            'dirs: for dir in dirs.iter().filter(|d| d.is_dir()) {
                let mut any = false;
                for stem in stems {
                    for ext in ["ttf", "otf"] {
                        let p = dir.join(format!("{stem}.{ext}"));
                        if p.is_file() && Font::install_file(&p).is_ok_and(|f| !f.is_empty()) {
                            any = true;
                            // One variable Archivo file holds every face.
                            if family == typeface::UI {
                                break 'dirs;
                            }
                            break;
                        }
                    }
                }
                if any {
                    break;
                }
            }
        }
    });
}
