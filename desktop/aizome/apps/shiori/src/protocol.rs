//! What Shiori and its PDFium worker say to each other, over the worker's
//! stdin and stdout: each frame is a little-endian `u32` length and that
//! much JSON, and a drawing's pixels follow its frame.
//!
//! Geometry is in points from a page's top left as it shows, turned by its
//! `/Rotate`: the worker does PDF's upward, rotated coordinates, so the app
//! never has to.

use std::io::{self, Read, Write};
use std::path::PathBuf;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

/// `[x0, y0, x1, y1]` on a page, in points from its top left as it shows.
pub type PageRect = [f32; 4];

/// The most a frame's JSON may be (a page's text is the largest).
const MAX_FRAME: usize = 256 << 20;

/// A drawing of part of a page: the page drawn `size` device pixels large,
/// and the part of that at `at`, `w` × `h`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TileKey {
    pub page: u32,
    pub size: (u32, u32),
    pub at: (u32, u32),
    pub w: u32,
    pub h: u32,
}

impl TileKey {
    pub fn bytes(&self) -> usize {
        self.w as usize * self.h as usize * 4
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Request {
    /// Open the document (once, first).
    Open { path: PathBuf, password: Option<String> },
    /// What to draw, most wanted first, in place of what was asked before
    /// and not yet drawn.
    Draw(Vec<TileKey>),
    /// A page's characters and where they are.
    Text(u32),
    /// A page's links.
    Links(u32),
    /// Look for `query` on `pages` in that order, in place of any search
    /// still going.
    Find { serial: u64, query: String, case: bool, pages: Vec<u32> },
    /// Stop looking.
    StopFind,
}

/// What the worker is doing now: what the app holds it to when it
/// crashes or takes too long.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Job {
    Open,
    Draw(TileKey),
    Text(u32),
    Links(u32),
    Find(u32),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Reply {
    Opened(Box<DocInfo>),
    OpenFailed(OpenError),
    /// Starting on a job.
    Working(Job),
    /// A drawing: `key.bytes()` of straight RGBA follow the frame.
    Drawn(TileKey),
    DrawFailed(TileKey),
    Text {
        page: u32,
        text: PageText,
    },
    Links {
        page: u32,
        links: Vec<Link>,
    },
    /// A page searched: each hit is the rectangles it covers (a line each).
    Found {
        serial: u64,
        page: u32,
        hits: Vec<Vec<PageRect>>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum OpenError {
    /// It has a password and none (or the wrong one) was given.
    Password { tried: bool },
    /// Not a PDF, or too damaged to read.
    Format(String),
    /// The file couldn't be read.
    Io(String),
    /// PDFium itself wasn't found.
    Library(String),
}

impl OpenError {
    pub fn message(&self) -> String {
        match self {
            OpenError::Password { .. } => "It's protected by a password.".into(),
            OpenError::Format(e) => format!("It isn't a PDF Shiori can read ({e})."),
            OpenError::Io(e) => e.clone(),
            OpenError::Library(e) => format!("PDFium couldn't be loaded: {e}"),
        }
    }
}

/// What a document is, as it opened.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct DocInfo {
    /// Each page's size in points, as it shows.
    pub pages: Vec<(f32, f32)>,
    /// Title, Author, Subject, Keywords, Creator, Producer, dates: those
    /// it has.
    pub meta: Vec<(String, String)>,
    pub version: String,
    pub outline: Vec<Outline>,
    pub encrypted: bool,
    pub forms: bool,
    /// Whether its author allows copying its text.
    pub can_copy: bool,
}

impl DocInfo {
    pub fn meta(&self, key: &str) -> Option<&str> {
        self.meta.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str()).filter(|v| !v.trim().is_empty())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Target {
    /// A place in the document: a page, and how far down it (points).
    Page {
        page: u32,
        y: Option<f32>,
    },
    Uri(String),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Outline {
    pub title: String,
    pub target: Option<Target>,
    pub children: Vec<Outline>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Link {
    pub rect: PageRect,
    pub target: Target,
}

/// A page's characters in reading order, and each one's box (all zero
/// for those PDFium made up: spaces between words, line ends).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PageText {
    pub chars: Vec<char>,
    pub boxes: Vec<PageRect>,
}

/// Send `msg`, then `pixels`.
pub fn write_frame<T: Serialize>(w: &mut impl Write, msg: &T, pixels: Option<&[u8]>) -> io::Result<()> {
    let json = serde_json::to_vec(msg).map_err(io::Error::other)?;
    w.write_all(&(json.len() as u32).to_le_bytes())?;
    w.write_all(&json)?;
    if let Some(p) = pixels {
        w.write_all(p)?;
    }
    w.flush()
}

/// The next frame; `None` at the end of the stream.
pub fn read_frame<T: DeserializeOwned>(r: &mut impl Read) -> io::Result<Option<T>> {
    let mut len = [0u8; 4];
    match r.read_exact(&mut len) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e),
    }
    let len = u32::from_le_bytes(len) as usize;
    if len > MAX_FRAME {
        return Err(io::Error::new(io::ErrorKind::InvalidData, format!("a frame of {len} bytes")));
    }
    let mut json = vec![0u8; len];
    r.read_exact(&mut json)?;
    serde_json::from_slice(&json).map(Some).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

/// The pixels after a `Drawn` frame.
pub fn read_pixels(r: &mut impl Read, key: &TileKey) -> io::Result<Vec<u8>> {
    let mut px = vec![0u8; key.bytes()];
    r.read_exact(&mut px)?;
    Ok(px)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_go_and_come_back() {
        let key = TileKey { page: 3, size: (100, 200), at: (0, 64), w: 2, h: 1 };
        let mut buf = Vec::new();
        write_frame(&mut buf, &Reply::Drawn(key), Some(&[1, 2, 3, 4, 5, 6, 7, 8])).unwrap();
        write_frame(&mut buf, &Reply::Working(Job::Text(2)), None).unwrap();
        let mut r = &buf[..];
        let Some(Reply::Drawn(k)) = read_frame(&mut r).unwrap() else { panic!() };
        assert_eq!(k, key);
        assert_eq!(read_pixels(&mut r, &k).unwrap(), vec![1, 2, 3, 4, 5, 6, 7, 8]);
        assert!(matches!(read_frame(&mut r).unwrap(), Some(Reply::Working(Job::Text(2)))));
        assert!(read_frame::<Reply>(&mut r).unwrap().is_none());
    }

    #[test]
    fn a_huge_length_is_refused() {
        let buf = (u32::MAX).to_le_bytes();
        assert!(read_frame::<Reply>(&mut &buf[..]).is_err());
    }
}
