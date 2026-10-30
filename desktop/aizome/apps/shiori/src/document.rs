//! What a document says besides its pages' drawing, read with hayro's
//! parser: its information dictionary, outline, destinations and links.
//! Geometry is in points of a page as it shows, from its top left (the
//! page's `initial_transform`, which turns and crops as hayro draws).

use std::collections::HashMap;

use hayro::hayro_interpret::util::TransformExt;
use hayro::kurbo::{Affine, Point};
use hayro_syntax::Pdf;
use hayro_syntax::object::{Array, Dict, Name, Object, ObjectIdentifier, Rect};
use hayro_syntax::page::Page;

use crate::protocol::{Link, Outline, PageRect, Target};

/// Deep or circular outlines and name trees (damaged files have them)
/// stop here.
const MOST_ENTRIES: usize = 20_000;
const DEEPEST: usize = 32;

/// A page's PDF space to its points as it shows.
pub fn shown(page: &Page<'_>) -> Affine {
    page.initial_transform(true).to_kurbo()
}

/// The rectangle `r` of a page's PDF space, as it shows.
pub fn shown_rect(to_page: Affine, r: Rect) -> PageRect {
    let b = to_page.transform_rect_bbox(hayro::kurbo::Rect::new(r.x0, r.y0, r.x1, r.y1));
    [b.x0 as f32, b.y0 as f32, b.x1 as f32, b.y1 as f32]
}

/// A document's structure, for its outline and links.
pub struct Structure {
    /// Each page's index, by its object.
    pages: HashMap<ObjectIdentifier, u32>,
    /// Each page's PDF space as it shows.
    shown: Vec<Affine>,
    /// Named destinations, read the first time one is needed.
    named: Option<HashMap<Vec<u8>, Object<'static>>>,
    catalog: Option<Dict<'static>>,
}

impl Structure {
    pub fn new(pdf: &'static Pdf) -> Structure {
        let mut pages = HashMap::new();
        let mut shown_ = Vec::new();
        for (i, page) in pdf.pages().iter().enumerate() {
            if let Some(id) = page.raw().obj_id() {
                pages.entry(id).or_insert(i as u32);
            }
            shown_.push(shown(page));
        }
        let catalog = pdf.xref().get::<Dict<'static>>(pdf.xref().root_id());
        Structure { pages, shown: shown_, named: None, catalog }
    }

    pub fn catalog(&self) -> Option<&Dict<'static>> {
        self.catalog.as_ref()
    }

    /// The document's outline (its bookmarks).
    pub fn outline(&mut self) -> Vec<Outline> {
        let first = self.catalog.as_ref().and_then(|c| c.get::<Dict<'_>>(b"Outlines")).and_then(|o| o.get(b"First"));
        let mut count = 0;
        self.outline_items(first, 0, &mut count)
    }

    fn outline_items(&mut self, first: Option<Dict<'static>>, depth: usize, count: &mut usize) -> Vec<Outline> {
        let mut out = Vec::new();
        let mut item = first;
        while let Some(i) = item {
            *count += 1;
            if *count > MOST_ENTRIES || depth > DEEPEST {
                break;
            }
            let children = self.outline_items(i.get(b"First"), depth + 1, count);
            let target = match i.get::<Object<'_>>(b"Dest") {
                Some(d) => self.destination(d, 0),
                None => i.get::<Dict<'_>>(b"A").and_then(|a| self.action(&a)),
            };
            let title =
                i.get::<hayro_syntax::object::String<'_>>(b"Title").map(|t| text_string(&t)).unwrap_or_default();
            out.push(Outline { title: title.trim().to_owned(), target, children });
            item = i.get(b"Next");
        }
        out
    }

    /// A page's links: their rectangles as it shows, and where they go.
    pub fn links(&mut self, page: &Page<'static>, index: u32) -> Vec<Link> {
        let Some(annots) = page.raw().get::<Array<'_>>(b"Annots") else { return Vec::new() };
        let to_page = self.shown.get(index as usize).copied().unwrap_or_else(|| shown(page));
        let mut out = Vec::new();
        for annot in annots.iter::<Dict<'static>>() {
            if annot.get::<Name<'_>>(b"Subtype").is_none_or(|s| s.as_ref() != b"Link") {
                continue;
            }
            let Some(rect) = annot.get::<Rect>(b"Rect") else { continue };
            let target = match annot.get::<Object<'_>>(b"Dest") {
                Some(d) => self.destination(d, 0),
                None => annot.get::<Dict<'_>>(b"A").and_then(|a| self.action(&a)),
            };
            if let Some(target) = target {
                out.push(Link { rect: shown_rect(to_page, rect), target });
            }
        }
        out
    }

    /// Where an action goes: a place in the document, or a web or mail
    /// address. Others (launching programs, other files, scripts) go
    /// nowhere.
    fn action(&mut self, a: &Dict<'static>) -> Option<Target> {
        let kind = a.get::<Name<'_>>(b"S")?;
        match kind.as_ref() {
            b"URI" => {
                let uri = a.get::<hayro_syntax::object::String<'_>>(b"URI")?;
                let uri = String::from_utf8_lossy(&uri).trim().to_owned();
                (!uri.is_empty()).then_some(Target::Uri(uri))
            }
            b"GoTo" => self.destination(a.get(b"D")?, 0),
            _ => None,
        }
    }

    /// Where a destination goes: an explicit one (`[page /XYZ left top
    /// zoom]` and the like), or one named in the catalog.
    fn destination(&mut self, d: Object<'static>, depth: usize) -> Option<Target> {
        match d {
            Object::Array(a) => self.explicit(&a),
            Object::Name(n) => self.named(n.as_ref(), depth),
            Object::String(s) => self.named(&s, depth),
            // A named destination's value may be a dictionary with `/D`.
            Object::Dict(dict) if depth < 4 => self.destination(dict.get(b"D")?, depth + 1),
            _ => None,
        }
    }

    fn named(&mut self, name: &[u8], depth: usize) -> Option<Target> {
        if depth > 4 {
            return None;
        }
        if self.named.is_none() {
            let mut named = HashMap::new();
            if let Some(catalog) = &self.catalog {
                // PDF 1.1's dictionary of names, then the name tree.
                if let Some(dests) = catalog.get::<Dict<'static>>(b"Dests") {
                    for key in dests.keys() {
                        if let Some(v) = dests.get::<Object<'static>>(key.as_ref()) {
                            named.insert(key.as_ref().to_vec(), v);
                        }
                    }
                }
                if let Some(tree) = catalog.get::<Dict<'_>>(b"Names").and_then(|n| n.get::<Dict<'static>>(b"Dests")) {
                    let mut count = 0;
                    name_tree(&tree, 0, &mut count, &mut named);
                }
            }
            self.named = Some(named);
        }
        let d = self.named.as_ref()?.get(name)?.clone();
        self.destination(d, depth + 1)
    }

    fn explicit(&self, a: &Array<'static>) -> Option<Target> {
        let mut items = a.iter::<Object<'static>>();
        let page = match items.next()? {
            Object::Dict(d) => *self.pages.get(&d.obj_id()?)?,
            // Not for this document's destinations, but common.
            Object::Number(n) => {
                let n = n.as_f64();
                (n >= 0.0 && (n as usize) < self.shown.len()).then_some(n as u32)?
            }
            _ => return None,
        };
        let kind = match items.next() {
            Some(Object::Name(n)) => n.as_ref().to_vec(),
            _ => Vec::new(),
        };
        let numbers: Vec<Option<f64>> = items
            .map(|o| match o {
                Object::Number(n) => Some(n.as_f64()),
                _ => None,
            })
            .collect();
        let at = |i: usize| numbers.get(i).copied().flatten();
        // Where the top of the view goes, and from how far left.
        let top = match kind.as_slice() {
            b"XYZ" => at(1).map(|top| (at(0).unwrap_or(0.0), top)),
            b"FitH" | b"FitBH" => at(0).map(|top| (0.0, top)),
            b"FitR" => at(3).map(|top| (at(0).unwrap_or(0.0), top)),
            _ => None,
        };
        let y = top.and_then(|(x, y)| {
            let to_page = self.shown.get(page as usize)?;
            let p = *to_page * Point::new(x, y);
            p.y.is_finite().then_some((p.y as f32).max(0.0))
        });
        Some(Target::Page { page, y })
    }
}

/// A name tree's entries (`/Names [key value …]`, under `/Kids`).
fn name_tree(node: &Dict<'static>, depth: usize, count: &mut usize, out: &mut HashMap<Vec<u8>, Object<'static>>) {
    if depth > DEEPEST || *count > MOST_ENTRIES {
        return;
    }
    if let Some(names) = node.get::<Array<'static>>(b"Names") {
        let mut items = names.iter::<Object<'static>>();
        while let (Some(key), Some(value)) = (items.next(), items.next()) {
            *count += 1;
            if *count > MOST_ENTRIES {
                return;
            }
            if let Object::String(key) = key {
                out.entry(key.to_vec()).or_insert(value);
            }
        }
    }
    if let Some(kids) = node.get::<Array<'static>>(b"Kids") {
        for kid in kids.iter::<Dict<'static>>() {
            name_tree(&kid, depth + 1, count, out);
        }
    }
}

/// A PDF text string: UTF-16 or UTF-8 after a byte order mark, otherwise
/// PDFDocEncoding.
pub fn text_string(bytes: &[u8]) -> String {
    if let Some(rest) = bytes.strip_prefix(&[0xfe, 0xff]) {
        let units = rest.as_chunks::<2>().0.iter().map(|u| u16::from_be_bytes(*u));
        return char::decode_utf16(units).map(|c| c.unwrap_or('\u{fffd}')).collect();
    }
    if let Some(rest) = bytes.strip_prefix(&[0xff, 0xfe]) {
        let units = rest.as_chunks::<2>().0.iter().map(|u| u16::from_le_bytes(*u));
        return char::decode_utf16(units).map(|c| c.unwrap_or('\u{fffd}')).collect();
    }
    if let Some(rest) = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]) {
        return String::from_utf8_lossy(rest).into_owned();
    }
    bytes.iter().map(|&b| pdf_doc_char(b)).collect()
}

/// A byte of PDFDocEncoding (ISO 32000-2, annex D.3).
fn pdf_doc_char(b: u8) -> char {
    const LOW: [char; 8] = ['˘', 'ˇ', 'ˆ', '˙', '˝', '˛', '˚', '˜'];
    const HIGH: [char; 32] = [
        '•', '†', '‡', '…', '—', '–', 'ƒ', '⁄', '‹', '›', '−', '‰', '„', '“', '”', '‘', '’', '‚', '™', 'ﬁ', 'ﬂ', 'Ł',
        'Œ', 'Š', 'Ÿ', 'Ž', 'ı', 'ł', 'œ', 'š', 'ž', '\u{fffd}',
    ];
    match b {
        0x18..=0x1f => LOW[usize::from(b - 0x18)],
        0x80..=0x9f => HIGH[usize::from(b - 0x80)],
        0xa0 => '€',
        _ => char::from(b),
    }
}

/// The information dictionary's entries, as Shiori's Get Info names them.
pub fn metadata(pdf: &Pdf) -> Vec<(String, String)> {
    let m = pdf.metadata();
    let fields = [
        ("Title", &m.title),
        ("Author", &m.author),
        ("Subject", &m.subject),
        ("Keywords", &m.keywords),
        ("Creator", &m.creator),
        ("Producer", &m.producer),
    ];
    let mut out: Vec<(String, String)> = fields
        .into_iter()
        .filter_map(|(k, v)| v.as_deref().map(|v| (k.to_owned(), text_string(v).trim().to_owned())))
        .filter(|(_, v)| !v.is_empty())
        .collect();
    let dates = [("CreationDate", &m.creation_date), ("ModificationDate", &m.modification_date)];
    for (k, d) in dates {
        if let Some(d) = d {
            out.push((
                k.to_owned(),
                format!("{:04}-{:02}-{:02} {:02}:{:02}", d.year, d.month, d.day, d.hour, d.minute),
            ));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_strings_decode() {
        assert_eq!(text_string(b"Plain"), "Plain");
        assert_eq!(text_string(&[0xfe, 0xff, 0x30, 0x42, 0x00, 0x41]), "あA");
        assert_eq!(text_string(&[0xef, 0xbb, 0xbf, 0xe6, 0xa0, 0x9e]), "栞");
        assert_eq!(text_string(&[0x93, b'q', 0x94, 0x84, 0xa0, 0xe9]), "ﬁqﬂ—€é");
        assert_eq!(text_string(&[0x18, 0x1f]), "˘˜");
    }
}
