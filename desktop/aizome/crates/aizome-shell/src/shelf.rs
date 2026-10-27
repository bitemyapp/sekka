//! The Shelf: drags parked in the Deskbar — files, folders, text, images
//! and links — kept across sessions in `$XDG_STATE_HOME/aizome/shelf/`
//! (files and folders by path, text and images by content).

use std::path::{Path, PathBuf};

use aizome_kit::Icon;
use serde::{Deserialize, Serialize};

use crate::look::Hue;

/// A parked thing.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum ShelfItem {
    File {
        path: PathBuf,
    },
    Folder {
        path: PathBuf,
    },
    Text {
        text: String,
    },
    Link {
        url: String,
    },
    /// An image's bytes, kept as a file in the shelf's folder.
    Image {
        file: String,
        name: String,
    },
}

impl ShelfItem {
    /// A dropped file, folder or link.
    pub fn from_uri(uri: &str) -> Option<ShelfItem> {
        if let Some(p) = uri.strip_prefix("file://") {
            let path = PathBuf::from(percent_decode(p));
            return Some(if path.is_dir() { ShelfItem::Folder { path } } else { ShelfItem::File { path } });
        }
        if uri.contains("://") || uri.starts_with("mailto:") {
            return Some(ShelfItem::Link { url: uri.to_owned() });
        }
        None
    }

    /// Dropped text (a link if it's only one).
    pub fn from_text(text: &str) -> ShelfItem {
        let t = text.trim();
        if crate::actions::looks_like_url(t) && t.contains("://") {
            ShelfItem::Link { url: t.to_owned() }
        } else {
            ShelfItem::Text { text: text.to_owned() }
        }
    }

    /// The row's text.
    pub fn label(&self) -> String {
        match self {
            ShelfItem::File { path } => name_of(path),
            ShelfItem::Folder { path } => format!("{}/", name_of(path)),
            ShelfItem::Text { text } => {
                let first: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
                let short: String = first.chars().take(24).collect();
                if first.chars().count() > 24 { format!("“{short}…”") } else { format!("“{short}”") }
            }
            ShelfItem::Link { url } => url.split("://").nth(1).unwrap_or(url).trim_end_matches('/').to_owned(),
            ShelfItem::Image { name, .. } => name.clone(),
        }
    }

    pub fn glyph(&self) -> (Icon, Hue) {
        match self {
            ShelfItem::File { path } => {
                let g = crate::palette::model::file_glyph(path);
                (g, if g == Icon::FileImage { Hue::Violet } else { Hue::Text2 })
            }
            ShelfItem::Folder { .. } => (Icon::Folder, Hue::Blue),
            ShelfItem::Text { .. } => (Icon::TextAlignStart, Hue::Green),
            ShelfItem::Link { .. } => (Icon::Link, Hue::Cyan),
            ShelfItem::Image { .. } => (Icon::Image, Hue::Violet),
        }
    }

    /// The text a drag out of the Shelf (or Copy) carries.
    pub fn as_text(&self, dir: &Path) -> String {
        match self {
            ShelfItem::File { path } | ShelfItem::Folder { path } => path.to_string_lossy().into_owned(),
            ShelfItem::Text { text } => text.clone(),
            ShelfItem::Link { url } => url.clone(),
            ShelfItem::Image { file, .. } => dir.join(file).to_string_lossy().into_owned(),
        }
    }

    /// The URI a drag out carries, for files and links.
    pub fn as_uri(&self, dir: &Path) -> Option<String> {
        match self {
            ShelfItem::File { path } | ShelfItem::Folder { path } => Some(file_url(path)),
            ShelfItem::Link { url } => Some(url.clone()),
            ShelfItem::Image { file, .. } => Some(file_url(&dir.join(file))),
            ShelfItem::Text { .. } => None,
        }
    }

    /// What Open opens.
    pub fn target(&self, dir: &Path) -> Option<PathBuf> {
        match self {
            ShelfItem::File { path } | ShelfItem::Folder { path } => Some(path.clone()),
            ShelfItem::Image { file, .. } => Some(dir.join(file)),
            _ => None,
        }
    }
}

fn name_of(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| p.display().to_string())
}

pub fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && i + 2 < b.len()
            && let Ok(v) = u8::from_str_radix(std::str::from_utf8(&b[i + 1..i + 3]).unwrap_or("zz"), 16)
        {
            out.push(v);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The Shelf's items and where they're kept.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Shelf {
    pub items: Vec<ShelfItem>,
    pub dir: PathBuf,
}

impl Shelf {
    pub fn default_dir() -> PathBuf {
        aizome_config::state_dir().join("shelf")
    }

    /// The Shelf kept in `dir`, or an empty one.
    pub fn load(dir: &Path) -> Shelf {
        let items = std::fs::read(dir.join("shelf.json"))
            .ok()
            .and_then(|b| serde_json::from_slice::<Vec<ShelfItem>>(&b).ok())
            .unwrap_or_default();
        Shelf { items, dir: dir.to_owned() }
    }

    pub fn save(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.dir)?;
        let tmp = self.dir.join("shelf.json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(&self.items)?)?;
        std::fs::rename(tmp, self.dir.join("shelf.json"))?;
        // Images no item holds any more go.
        if let Ok(rd) = std::fs::read_dir(&self.dir) {
            for e in rd.flatten() {
                let name = e.file_name().to_string_lossy().into_owned();
                if name.starts_with("image-")
                    && !self.items.iter().any(|i| matches!(i, ShelfItem::Image { file, .. } if *file == name))
                {
                    let _ = std::fs::remove_file(e.path());
                }
            }
        }
        Ok(())
    }

    /// Park an item (moving it to the end if it's there already).
    pub fn park(&mut self, item: ShelfItem) {
        self.items.retain(|i| *i != item);
        self.items.push(item);
    }

    /// Park an image's bytes.
    pub fn park_image(&mut self, bytes: &[u8], name: &str, ext: &str) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.dir)?;
        let stamp =
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
        let file = format!("image-{stamp}.{ext}");
        std::fs::write(self.dir.join(&file), bytes)?;
        self.park(ShelfItem::Image { file, name: name.to_owned() });
        Ok(())
    }

    pub fn remove(&mut self, index: usize) -> Option<ShelfItem> {
        (index < self.items.len()).then(|| self.items.remove(index))
    }
}

/// A path as a `file://` URL, what isn't plain encoded (`%20`).
pub fn file_url(path: &Path) -> String {
    let mut s = String::from("file://");
    for b in path.to_string_lossy().bytes() {
        if b.is_ascii_alphanumeric() || b"/-._~".contains(&b) {
            s.push(b as char);
        } else {
            s.push_str(&format!("%{b:02X}"));
        }
    }
    s
}

/// What dragging an item out of the Shelf carries: its URL (files and
/// links), and its text.
pub fn drag_source(uri: Option<&str>, text: &str) -> aizome_kit::sidestep_ui::DragSource {
    let mut s = aizome_kit::sidestep_ui::DragSource::new();
    if let Some(u) = uri {
        s = s.urls([u]);
    }
    s.text(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drops_become_items() {
        assert_eq!(
            ShelfItem::from_uri("file:///tmp/release%20notes.md"),
            Some(ShelfItem::File { path: "/tmp/release notes.md".into() })
        );
        assert_eq!(
            ShelfItem::from_uri("https://aizome.dev"),
            Some(ShelfItem::Link { url: "https://aizome.dev".into() })
        );
        assert_eq!(
            ShelfItem::from_text("https://example.org/x"),
            ShelfItem::Link { url: "https://example.org/x".into() }
        );
        let t = ShelfItem::from_text("damage is per layer, not per window");
        assert_eq!(t.label(), "“damage is per layer, not…”");
        assert_eq!(ShelfItem::Folder { path: "/home/u/release-notes".into() }.label(), "release-notes/");
    }

    #[test]
    fn the_shelf_survives_a_restart() {
        let dir = std::env::temp_dir().join(format!("aizome-shelf-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut s = Shelf::load(&dir);
        assert!(s.items.is_empty());
        s.park(ShelfItem::Text { text: "hello".into() });
        s.park_image(b"\x89PNG", "wallpaper-draft.png", "png").unwrap();
        s.park(ShelfItem::Text { text: "hello".into() });
        assert_eq!(s.items.len(), 2);
        assert!(matches!(s.items[1], ShelfItem::Text { .. }), "parking again moves it to the end");
        s.save().unwrap();
        let back = Shelf::load(&dir);
        assert_eq!(back.items, s.items);
        let mut back = back;
        back.remove(0);
        back.save().unwrap();
        assert!(
            !std::fs::read_dir(&dir).unwrap().flatten().any(|e| e.file_name().to_string_lossy().starts_with("image-"))
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
