//! App icons: app_id → .desktop file → `Icon=` → a file in the icon
//! theme (hicolor and the configured theme, PNG or SVG), rasterized once
//! per size and cached as textures.
//!
//! Tabs show a line glyph in the tab's ink colour, as the boards draw
//! there: Lucide's, for Aizome's apps and the common ones the shell's
//! Deskbar knows (the same glyphs), else the app's icon as a silhouette.
//! The switcher shows the icon in colour.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use aizome_theme::Rgba;
use sidestep_ui::kurbo::{Rect as KRect, Size};
use sidestep_ui::{Appearance, Color, Image, StrokeStyle};
use smithay::backend::allocator::Fourcc;
use smithay::backend::renderer::element::Kind;
use smithay::backend::renderer::element::memory::MemoryRenderBuffer;
use smithay::utils::{Logical, Point, Transform};

use crate::scene::SceneItem;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Tint {
    /// The icon's own colours.
    Color,
    /// Its silhouette in one colour.
    Ink(Rgba),
}

pub struct IconTexture {
    pub buffer: MemoryRenderBuffer,
    /// Size in points.
    pub size: i32,
}

#[derive(Default)]
pub struct Icons {
    /// app_id → icon file.
    files: HashMap<String, Option<PathBuf>>,
    /// Desktop file IDs and StartupWMClass values → desktop files.
    desktop: Option<HashMap<String, PathBuf>>,
    textures: HashMap<(String, i32, u32, Tint), Option<IconTexture>>,
    pub theme: Option<String>,
}

impl Icons {
    pub fn new() -> Icons {
        Icons::default()
    }

    /// Forget what was found (the icon theme or installed apps changed).
    pub fn clear(&mut self) {
        self.files.clear();
        self.desktop = None;
        self.textures.clear();
    }

    /// The app's display name from its desktop file, if any.
    pub fn app_name(&mut self, app_id: &str) -> Option<String> {
        let path = self.desktop_file(app_id)?;
        desktop_key(&path, "Name")
    }

    fn desktop_file(&mut self, app_id: &str) -> Option<PathBuf> {
        if self.desktop.is_none() {
            self.desktop = Some(index_desktop_files());
        }
        let idx = self.desktop.as_ref()?;
        let lower = app_id.to_lowercase();
        idx.get(&lower).or_else(|| idx.get(lower.rsplit('.').next().unwrap_or(&lower))).cloned()
    }

    /// The icon file for an app, if one is installed.
    pub fn file(&mut self, app_id: &str) -> Option<PathBuf> {
        if let Some(f) = self.files.get(app_id) {
            return f.clone();
        }
        let icon =
            self.desktop_file(app_id).and_then(|d| desktop_key(&d, "Icon")).unwrap_or_else(|| app_id.to_string());
        let found = find_icon(&icon, 64, self.theme.as_deref())
            .or_else(|| find_icon(&app_id.to_lowercase(), 64, self.theme.as_deref()));
        self.files.insert(app_id.to_string(), found.clone());
        found
    }

    /// An app's icon `size` points square at an integer scale.
    pub fn texture(&mut self, app_id: &str, size: i32, scale: u32, tint: Tint) -> Option<&IconTexture> {
        let key = (app_id.to_string(), size, scale, tint);
        if !self.textures.contains_key(&key) {
            let line = match tint {
                Tint::Ink(_) => line_glyph(app_id).and_then(|svg| {
                    let (w, h, rgba) = crate::svg::rasterize_data(svg.as_bytes(), (size as u32 * scale).max(16))?;
                    tinted(w, h, rgba, size, scale, tint)
                }),
                Tint::Color => None,
            };
            let file = if line.is_some() { None } else { self.file(app_id) };
            let tex = line.or_else(|| file.and_then(|f| rasterize(&f, size, scale, tint))).or_else(|| match tint {
                Tint::Ink(c) => Some(generic_glyph(size, scale, c)),
                Tint::Color => Some(generic_tile(app_id, size, scale)),
            });
            self.textures.insert(key.clone(), tex);
        }
        self.textures.get(&key).and_then(|t| t.as_ref())
    }
}

/// The size of the glyph an idle tab shows.
pub const GLYPH: f64 = 13.0;

/// The glyph an idle tab shows in place of its close box.
pub fn glyph_item(
    icons: &mut Icons,
    app_id: &str,
    ink: Rgba,
    at: (f64, f64),
    origin: (f64, f64),
    scale: f64,
    alpha: f32,
) -> Option<SceneItem> {
    let size = GLYPH as i32;
    let tex = icons.texture(app_id, size, scale.ceil().max(1.0) as u32, Tint::Ink(ink))?;
    Some(icon_item(tex, at, origin, scale, alpha))
}

pub fn icon_item(tex: &IconTexture, at: (f64, f64), origin: (f64, f64), scale: f64, alpha: f32) -> SceneItem {
    let loc = Point::<f64, Logical>::from((at.0 - origin.0, at.1 - origin.1)).to_physical_precise_round(scale);
    SceneItem::Memory {
        buffer: tex.buffer.clone(),
        loc: smithay::utils::Point::<i32, smithay::utils::Physical>::to_f64(loc),
        alpha: Some(alpha),
        src: None,
        size: Some((tex.size, tex.size).into()),
        kind: Kind::Unspecified,
    }
}

fn to_buffer(image: Image, size: i32, scale: u32) -> Option<IconTexture> {
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
    Some(IconTexture { buffer, size })
}

/// The Lucide glyph an app's tab shows, by the last part of its app ID, as
/// the shell's Deskbar picks them (`aizome-shell`'s `apps::catalog::glyph_for`).
fn line_glyph(app_id: &str) -> Option<&'static str> {
    macro_rules! lucide {
        ($name:literal) => {
            include_str!(concat!("../../aizome-kit/icons/lucide/", $name, ".svg"))
        };
    }
    let id = app_id.to_ascii_lowercase();
    let id = id.strip_suffix(".desktop").unwrap_or(&id);
    Some(match id.rsplit('.').next().unwrap_or(id) {
        "settei" => lucide!("settings"),
        "yukimi" => lucide!("package"),
        "hako" | "nautilus" | "thunar" | "dolphin" | "pcmanfm" | "nemo" => lucide!("folder"),
        "sumi" | "code" | "codium" | "zed" | "helix" | "nvim" | "emacs" | "gedit" => lucide!("code"),
        "fusen" => lucide!("sticky-note"),
        "koyomi" => lucide!("calendar-days"),
        "tokei" => lucide!("clock"),
        "soroban" => lucide!("calculator"),
        "tenki" => lucide!("cloud-sun"),
        "tansu" => lucide!("hard-drive"),
        "kura" => lucide!("chart-pie"),
        "myaku" => lucide!("activity"),
        "shashin" | "imv" | "eog" | "loupe" => lucide!("image"),
        "annai" => lucide!("compass"),
        "foot" | "footclient" | "alacritty" | "kitty" | "wezterm" | "ghostty" | "konsole" | "terminal" | "xterm" => {
            lucide!("square-terminal")
        }
        "firefox" | "chromium" | "chromium-browser" | "google-chrome" | "brave-browser" | "epiphany" | "librewolf" => {
            lucide!("globe")
        }
        "thunderbird" | "evolution" | "geary" | "mail" => lucide!("mail"),
        "mpv" | "vlc" | "totem" => lucide!("film"),
        "gimp" | "inkscape" | "krita" => lucide!("paintbrush"),
        _ => return None,
    })
}

fn rasterize(path: &Path, size: i32, scale: u32, tint: Tint) -> Option<IconTexture> {
    let (w, h, rgba) = load_rgba(path, (size as u32 * scale).max(16))?;
    tinted(w, h, rgba, size, scale, tint)
}

/// Straight RGBA, in one colour if `tint` asks, drawn `size` points square.
fn tinted(w: u32, h: u32, rgba: Vec<u8>, size: i32, scale: u32, tint: Tint) -> Option<IconTexture> {
    let mut rgba = rgba;
    if let Tint::Ink(c) = tint {
        for p in rgba.as_chunks_mut::<4>().0 {
            p[0] = c.r;
            p[1] = c.g;
            p[2] = c.b;
            p[3] = ((p[3] as u16 * c.a as u16) / 255) as u8;
        }
    }
    let img = Image::from_rgba(w, h, &rgba)?;
    let s = size as f64;
    let out = sidestep_ui::offscreen::render(Size::new(s, s), scale as f64, Appearance::DARK, |c| {
        c.draw_image(&img, KRect::new(0.0, 0.0, s, s));
    });
    to_buffer(out, size, scale)
}

/// Decodes a PNG (or rasterizes an SVG at `px`) to straight RGBA.
fn load_rgba(path: &Path, px: u32) -> Option<(u32, u32, Vec<u8>)> {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
    if ext == "svg" {
        return crate::svg::rasterize(path, px);
    }
    let file = std::fs::File::open(path).ok()?;
    let mut decoder = png::Decoder::new(std::io::BufReader::new(file));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().ok()?;
    let mut buf = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buf).ok()?;
    let data = &buf[..info.buffer_size()];
    let rgba: Vec<u8> = match info.color_type {
        png::ColorType::Rgba => data.to_vec(),
        png::ColorType::Rgb => data.as_chunks::<3>().0.iter().flat_map(|p| [p[0], p[1], p[2], 255]).collect(),
        png::ColorType::GrayscaleAlpha => {
            data.as_chunks::<2>().0.iter().flat_map(|p| [p[0], p[0], p[0], p[1]]).collect()
        }
        png::ColorType::Grayscale => data.iter().flat_map(|g| [*g, *g, *g, 255]).collect(),
        png::ColorType::Indexed => return None,
    };
    Some((info.width, info.height, rgba))
}

/// A window glyph for apps without an icon: a small framed window.
fn generic_glyph(size: i32, scale: u32, ink: Rgba) -> IconTexture {
    let s = size as f64;
    let color = Color::rgba8(ink.r, ink.g, ink.b, ink.a);
    let img = sidestep_ui::offscreen::render(Size::new(s, s), scale as f64, Appearance::DARK, |c| {
        let r = KRect::new(1.5, 2.5, s - 1.5, s - 2.5);
        c.stroke(&r, &StrokeStyle::new(1.3), color);
        c.fill_rect(KRect::new(1.5, 2.5, s - 1.5, 5.0), color);
    });
    to_buffer(img, size, scale).expect("rasterized glyph")
}

/// A coloured tile with the app's initial, for the switcher.
fn generic_tile(app_id: &str, size: i32, scale: u32) -> IconTexture {
    let s = size as f64;
    let hue = app_id.bytes().fold(0u32, |h, b| h.wrapping_mul(31).wrapping_add(b as u32));
    let palette = [0x7aa2f7, 0x7dcfff, 0x9ece6a, 0xbb9af7, 0xe0af68, 0xf7768e];
    let bg = Rgba::hex(palette[(hue as usize) % palette.len()]);
    let initial: String =
        app_id.rsplit('.').next().unwrap_or(app_id).chars().take(1).collect::<String>().to_uppercase();
    let img = sidestep_ui::offscreen::render(Size::new(s, s), scale as f64, Appearance::DARK, |c| {
        c.fill_rect(KRect::new(0.0, 0.0, s, s), Color::rgba8(bg.r, bg.g, bg.b, 255));
        let font = sidestep_ui::Font::named(aizome_theme::typeface::UI, s * 0.55)
            .unwrap_or_else(|| sidestep_ui::Font::system(s * 0.55))
            .with_weight(700.0);
        let style = sidestep_ui::TextStyle::new(font, Color::rgba8(0x1a, 0x1b, 0x26, 255));
        let m = sidestep_ui::measure(&initial, &style);
        c.draw_label(&initial, &style, sidestep_ui::kurbo::Point::new((s - m.width) / 2.0, (s - m.height) / 2.0));
    });
    to_buffer(img, size, scale).expect("rasterized tile")
}

fn data_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    let home = aizome_config::home_dir();
    dirs.push(std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).unwrap_or_else(|| home.join(".local/share")));
    let sys = std::env::var("XDG_DATA_DIRS").unwrap_or_else(|_| "/usr/local/share:/usr/share".into());
    dirs.extend(sys.split(':').filter(|s| !s.is_empty()).map(PathBuf::from));
    for extra in ["/run/current-system/sw/share", "/etc/profiles/per-user"] {
        let p = PathBuf::from(extra);
        if !dirs.contains(&p) && p.exists() {
            dirs.push(p);
        }
    }
    dirs
}

/// Desktop file IDs, their basenames' last components and their
/// StartupWMClass, lowercased, to files (earlier directories win).
fn index_desktop_files() -> HashMap<String, PathBuf> {
    let mut idx = HashMap::new();
    for dir in data_dirs() {
        let Ok(entries) = std::fs::read_dir(dir.join("applications")) else { continue };
        for e in entries.flatten() {
            let path = e.path();
            if path.extension().and_then(|x| x.to_str()) != Some("desktop") {
                continue;
            }
            let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_lowercase();
            idx.entry(stem.clone()).or_insert_with(|| path.clone());
            if let Some(last) = stem.rsplit('.').next() {
                idx.entry(last.to_string()).or_insert_with(|| path.clone());
            }
            if let Some(class) = desktop_key(&path, "StartupWMClass") {
                idx.entry(class.to_lowercase()).or_insert_with(|| path.clone());
            }
        }
    }
    idx
}

/// A key from a desktop file's `[Desktop Entry]` group.
pub fn desktop_key(path: &Path, key: &str) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    let mut in_entry = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
            continue;
        }
        if !in_entry {
            continue;
        }
        if let Some((k, v)) = line.split_once('=')
            && k.trim() == key
        {
            return Some(v.trim().to_string());
        }
    }
    None
}

/// Finds an icon by name in the icon themes, nearest `size` first.
pub fn find_icon(name: &str, size: u32, theme: Option<&str>) -> Option<PathBuf> {
    if name.is_empty() {
        return None;
    }
    let p = Path::new(name);
    if p.is_absolute() {
        return p.exists().then(|| p.to_path_buf());
    }
    let mut themes: Vec<&str> = Vec::new();
    if let Some(t) = theme {
        themes.push(t);
    }
    themes.extend(["hicolor", "Adwaita", "Papirus", "breeze"]);
    let mut sizes: Vec<u32> = vec![size, 64, 48, 128, 96, 256, 32, 512, 24, 22, 16];
    sizes.dedup();
    let dirs = data_dirs();
    let mut roots: Vec<PathBuf> = vec![aizome_config::home_dir().join(".icons")];
    roots.extend(dirs.iter().map(|d| d.join("icons")));
    for theme in &themes {
        for root in &roots {
            let base = root.join(theme);
            if !base.exists() {
                continue;
            }
            for s in &sizes {
                for sub in [format!("{s}x{s}/apps"), format!("{s}x{s}@2/apps")] {
                    let f = base.join(&sub).join(format!("{name}.png"));
                    if f.exists() {
                        return Some(f);
                    }
                }
            }
            for sub in ["scalable/apps", "symbolic/apps"] {
                let f = base.join(sub).join(format!("{name}.svg"));
                if f.exists() {
                    return Some(f);
                }
            }
        }
    }
    for d in &dirs {
        for ext in ["png", "svg"] {
            let f = d.join("pixmaps").join(format!("{name}.{ext}"));
            if f.exists() {
                return Some(f);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_desktop_keys() {
        let dir = std::env::temp_dir().join(format!("aizome-icons-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("org.example.App.desktop");
        std::fs::write(&f, "[Desktop Entry]\nName=Example\nIcon=example\n[Desktop Action new]\nIcon=other\n").unwrap();
        assert_eq!(desktop_key(&f, "Icon").as_deref(), Some("example"));
        assert_eq!(desktop_key(&f, "Name").as_deref(), Some("Example"));
        assert_eq!(desktop_key(&f, "Exec"), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn tabs_draw_line_glyphs_not_silhouettes() {
        // Aizome's apps and foot get Lucide's glyphs, drawn as lines: an
        // icon tile's silhouette would be a solid square.
        for id in ["org.aizome.Sumi", "org.aizome.Tokei", "foot", "org.mozilla.firefox"] {
            let svg = line_glyph(id).unwrap_or_else(|| panic!("a glyph for {id}"));
            let (w, h, rgba) = crate::svg::rasterize_data(svg.as_bytes(), 26).expect("it rasterizes");
            let covered = rgba.as_chunks::<4>().0.iter().filter(|p| p[3] > 128).count();
            assert!(covered * 2 < (w * h) as usize, "{id}: {covered} of {} pixels inked", w * h);
            assert!(covered > 20, "{id}: something is drawn");
        }
        assert!(line_glyph("org.example.Unknown").is_none());
    }
}
