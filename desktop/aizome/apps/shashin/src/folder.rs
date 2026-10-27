//! The pictures in a folder, in Hako's order: by name, naturally (`IMG_2`
//! before `IMG_10`), letters without case.

use std::cmp::Ordering;
use std::path::{Path, PathBuf};

/// The file name endings Shashin opens.
pub const EXTENSIONS: &[&str] =
    &["png", "apng", "jpg", "jpeg", "jpe", "jfif", "gif", "webp", "bmp", "dib", "tif", "tiff", "ico", "svg", "svgz"];

/// Whether `path` is named as a picture Shashin opens.
pub fn is_picture(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| EXTENSIONS.iter().any(|x| x.eq_ignore_ascii_case(e)))
}

/// Whether the name starts with a dot.
pub fn is_hidden(path: &Path) -> bool {
    path.file_name().is_some_and(|n| n.as_encoded_bytes().first() == Some(&b'.'))
}

/// The pictures in `dir`, sorted; hidden ones only with `hidden`. Folders
/// and what can't be read are left out.
pub fn list(dir: &Path, hidden: bool) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut out: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| is_picture(p) && (hidden || !is_hidden(p)))
        // Through links: a link to a picture is a picture.
        .filter(|p| std::fs::metadata(p).is_ok_and(|m| m.is_file()))
        .collect();
    sort(&mut out);
    out
}

/// Sort paths as Hako sorts a folder by name.
pub fn sort(paths: &mut [PathBuf]) {
    paths.sort_by_cached_key(|p| {
        let name = display_name(p);
        (natural_key(&name), name)
    });
}

/// The name to show and sort by.
pub fn display_name(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| path.display().to_string())
}

/// A piece of a name for natural sorting: runs of digits compare as
/// numbers, letters without case (Hako's `model::entry`).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Chunk {
    Number(u128, usize),
    Text(String),
}

pub fn natural_key(s: &str) -> Vec<Chunk> {
    let mut out = Vec::new();
    let mut digits = String::new();
    let mut text = String::new();
    for c in s.chars() {
        if c.is_ascii_digit() {
            if !text.is_empty() {
                out.push(Chunk::Text(std::mem::take(&mut text)));
            }
            digits.push(c);
        } else {
            if !digits.is_empty() {
                let d = std::mem::take(&mut digits);
                out.push(Chunk::Number(d.parse().unwrap_or(u128::MAX), d.len()));
            }
            text.extend(c.to_lowercase());
        }
    }
    if !digits.is_empty() {
        out.push(Chunk::Number(digits.parse().unwrap_or(u128::MAX), digits.len()));
    }
    if !text.is_empty() {
        out.push(Chunk::Text(text));
    }
    out
}

/// Compare names naturally.
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    natural_key(a).cmp(&natural_key(b)).then_with(|| a.cmp(b))
}

/// Where `path` goes among `paths` (sorted), to put it back.
pub fn insertion_point(paths: &[PathBuf], path: &Path) -> usize {
    let name = display_name(path);
    paths.partition_point(|p| natural_cmp(&display_name(p), &name) == Ordering::Less)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sorts_as_hako_does() {
        let mut v: Vec<PathBuf> =
            ["IMG_10.jpg", "img_2.png", "IMG_1.JPG", "b.gif", "A.webp"].iter().map(PathBuf::from).collect();
        sort(&mut v);
        let names: Vec<String> = v.iter().map(|p| display_name(p)).collect();
        assert_eq!(names, ["A.webp", "b.gif", "IMG_1.JPG", "img_2.png", "IMG_10.jpg"]);
        assert_eq!(insertion_point(&v, Path::new("IMG_3.jpg")), 4);
    }

    #[test]
    fn knows_pictures_by_name() {
        assert!(is_picture(Path::new("/a/B.JPG")));
        assert!(is_picture(Path::new("logo.svgz")));
        assert!(!is_picture(Path::new("notes.txt")));
        assert!(!is_picture(Path::new("jpg")));
        assert!(is_hidden(Path::new("/x/.secret.png")));
    }
}
