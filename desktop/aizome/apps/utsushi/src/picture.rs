//! A picture taken from the video: PNG for the clipboard, or a file in
//! the Pictures folder's Screenshots.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use aizome_media::Picture;
use sidestep_engine::clipboard::{self, Contents};

/// The picture as PNG.
pub fn encode(pic: &Picture) -> Option<Vec<u8>> {
    let img = image::RgbaImage::from_raw(pic.width, pic.height, pic.rgba.clone())?;
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png).ok()?;
    Some(out.into_inner())
}

/// Offer `png` as the clipboard's contents, as `image/png`.
pub fn offer(png: Vec<u8>) {
    let shared = clipboard::shared();
    shared.bump();
    let data: Arc<[u8]> = png.into();
    shared.offer(Some(Contents { items: vec![("image/png".to_owned(), Some(data))] }));
}

/// The Pictures folder: `XDG_PICTURES_DIR` from `user-dirs.dirs`, else
/// `~/Pictures`.
pub fn pictures_dir() -> Option<PathBuf> {
    let home = PathBuf::from(std::env::var_os("HOME")?);
    let config = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).unwrap_or_else(|| home.join(".config"));
    let dirs = std::fs::read_to_string(config.join("user-dirs.dirs")).unwrap_or_default();
    let named = dirs.lines().find_map(|l| l.strip_prefix("XDG_PICTURES_DIR=")).map(|v| {
        let v = v.trim().trim_matches('"');
        PathBuf::from(v.replace("$HOME", &home.to_string_lossy()))
    });
    Some(named.unwrap_or_else(|| home.join("Pictures")))
}

/// Save `png` as `name` in Pictures/Screenshots (a number added if it's
/// taken): where it went.
pub fn save(name: &str, png: &[u8]) -> Result<PathBuf, String> {
    let dir = pictures_dir().ok_or("There's no home folder.")?.join("Screenshots");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let name = name.replace('/', "-");
    let mut path = dir.join(&name);
    let stem = Path::new(&name).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let mut n = 2;
    while path.exists() {
        path = dir.join(format!("{stem} ({n}).png"));
        n += 1;
    }
    std::fs::write(&path, png).map_err(|e| e.to_string())?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pictures_become_pngs() {
        let pic = Picture { width: 2, height: 1, rgba: vec![255, 0, 0, 255, 0, 0, 255, 255] };
        let png = encode(&pic).expect("encoded");
        assert_eq!(&png[1..4], b"PNG");
        let back = image::load_from_memory(&png).expect("decoded").to_rgba8();
        assert_eq!(back.as_raw(), &pic.rgba);
    }
}
