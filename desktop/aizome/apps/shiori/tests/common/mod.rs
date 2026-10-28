//! Documents to test Shiori with, written here rather than kept as files:
//! pages of text in the standard fonts, links within the document and out
//! of it, an outline, a landscape page, a turned one and one cropped.

#![allow(dead_code)]

use std::path::{Path, PathBuf};

pub fn scratch(name: &str) -> PathBuf {
    let base = option_env!("CARGO_TARGET_TMPDIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    let d = base.join(format!("shiori-{name}"));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// The worker: this package's own program.
pub fn worker() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_aizome-shiori"))
}

/// A page to write: its size, turn, crop box, text runs (x, y up from the
/// bottom, size, text) and links (rect, and a page index or a URI).
pub struct Page {
    pub size: (f32, f32),
    pub rotate: u32,
    pub crop: Option<[f32; 4]>,
    pub runs: Vec<(f32, f32, f32, String)>,
    pub links: Vec<([f32; 4], LinkTo)>,
    /// Filled rectangles (x, y, w, h, gray 0–1).
    pub boxes: Vec<(f32, f32, f32, f32, f32)>,
}

pub enum LinkTo {
    Page(usize),
    Uri(String),
}

impl Page {
    pub fn letter() -> Page {
        Page { size: (612.0, 792.0), rotate: 0, crop: None, runs: Vec::new(), links: Vec::new(), boxes: Vec::new() }
    }

    pub fn text(mut self, x: f32, y: f32, size: f32, t: &str) -> Page {
        self.runs.push((x, y, size, t.to_owned()));
        self
    }
}

/// An outline entry: its title, page and children.
pub struct Mark(pub &'static str, pub usize, pub Vec<Mark>);

fn esc(s: &str) -> String {
    s.replace('\\', "\\\\").replace('(', "\\(").replace(')', "\\)")
}

/// The bytes of a PDF of `pages` with `outline`.
pub fn pdf(pages: &[Page], outline: &[Mark]) -> Vec<u8> {
    // Objects: 1 catalog, 2 pages, 3 font, then per page (page, contents),
    // then the outline's.
    let n = pages.len();
    let page_obj = |i: usize| 4 + 2 * i;
    let mut objs: Vec<String> = Vec::new();
    let kids: Vec<String> = (0..n).map(|i| format!("{} 0 R", page_obj(i))).collect();
    let first_outline = 4 + 2 * n;
    // The outline's objects, numbered depth first after the pages'.
    let mut outline_objs: Vec<(usize, String)> = Vec::new();
    fn count(marks: &[Mark]) -> usize {
        marks.iter().map(|m| 1 + count(&m.2)).sum()
    }
    let has_outline = !outline.is_empty();
    let outline_root = first_outline;
    fn lay(
        marks: &[Mark],
        parent: usize,
        next_free: &mut usize,
        out: &mut Vec<(usize, String)>,
        page_obj: &dyn Fn(usize) -> usize,
    ) -> (usize, usize) {
        let ids: Vec<usize> = marks
            .iter()
            .map(|_| {
                let id = *next_free;
                *next_free += 1;
                id
            })
            .collect();
        for (k, m) in marks.iter().enumerate() {
            let id = ids[k];
            let mut d =
                format!("<< /Title ({}) /Parent {parent} 0 R /Dest [{} 0 R /XYZ 0 792 0]", esc(m.0), page_obj(m.1));
            if k > 0 {
                d += &format!(" /Prev {} 0 R", ids[k - 1]);
            }
            if k + 1 < ids.len() {
                d += &format!(" /Next {} 0 R", ids[k + 1]);
            }
            if !m.2.is_empty() {
                let (first, last) = lay(&m.2, id, next_free, out, page_obj);
                d += &format!(" /First {first} 0 R /Last {last} 0 R /Count {}", count(&m.2));
            }
            d += " >>";
            out.push((id, d));
        }
        (ids[0], ids[ids.len() - 1])
    }
    if has_outline {
        let mut next_free = outline_root + 1;
        let (first, last) = lay(outline, outline_root, &mut next_free, &mut outline_objs, &page_obj);
        outline_objs.push((
            outline_root,
            format!("<< /Type /Outlines /First {first} 0 R /Last {last} 0 R /Count {} >>", count(outline)),
        ));
    }
    let catalog = if has_outline {
        format!("<< /Type /Catalog /Pages 2 0 R /Outlines {outline_root} 0 R /PageMode /UseOutlines >>")
    } else {
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned()
    };
    objs.push(catalog);
    objs.push(format!("<< /Type /Pages /Kids [{}] /Count {n} >>", kids.join(" ")));
    objs.push("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>".to_owned());
    for p in pages.iter() {
        let mut annots = Vec::new();
        for (r, to) in &p.links {
            let action = match to {
                LinkTo::Page(t) => format!("/Dest [{} 0 R /XYZ 0 792 0]", page_obj(*t)),
                LinkTo::Uri(u) => format!("/A << /S /URI /URI ({}) >>", esc(u)),
            };
            annots.push(format!(
                "<< /Type /Annot /Subtype /Link /Rect [{} {} {} {}] /Border [0 0 0] {action} >>",
                r[0], r[1], r[2], r[3]
            ));
        }
        let mut d = format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {} {}] /Resources << /Font << /F1 3 0 R >> >> /Contents {} 0 R",
            p.size.0,
            p.size.1,
            objs.len() + 2
        );
        if p.rotate != 0 {
            d += &format!(" /Rotate {}", p.rotate);
        }
        if let Some(c) = p.crop {
            d += &format!(" /CropBox [{} {} {} {}]", c[0], c[1], c[2], c[3]);
        }
        if !annots.is_empty() {
            d += &format!(" /Annots [{}]", annots.join(" "));
        }
        d += " >>";
        objs.push(d);
        let mut content = String::new();
        for (x, y, w, h, g) in &p.boxes {
            content += &format!("{g} g {x} {y} {w} {h} re f 0 g\n");
        }
        for (x, y, size, t) in &p.runs {
            content += &format!("BT /F1 {size} Tf {x} {y} Td ({}) Tj ET\n", esc(t));
        }
        objs.push(format!("<< /Length {} >>\nstream\n{content}endstream", content.len()));
    }
    outline_objs.sort_by_key(|(id, _)| *id);
    for (id, d) in outline_objs {
        assert_eq!(id, objs.len() + 1);
        objs.push(d);
    }
    let mut out = b"%PDF-1.7\n%\xe2\xe3\xcf\xd3\n".to_vec();
    let mut offsets = Vec::new();
    for (i, o) in objs.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{o}\nendobj\n", i + 1).as_bytes());
    }
    let xref = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).as_bytes());
    for o in offsets {
        out.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objs.len() + 1).as_bytes(),
    );
    out
}

/// The document most tests read: four pages (letter, letter, landscape,
/// letter turned a quarter), "needle" on three of them, links on the
/// first, and an outline.
pub fn sample() -> Vec<u8> {
    let mut first = Page::letter()
        .text(72.0, 720.0, 24.0, "Shiori reads documents.")
        .text(72.0, 680.0, 14.0, "The second line has a needle in it.")
        .text(72.0, 604.0, 14.0, "Go to page three")
        .text(72.0, 564.0, 14.0, "Visit sekka.computer");
    first.links.push(([70.0, 598.0, 300.0, 622.0], LinkTo::Page(2)));
    first.links.push(([70.0, 558.0, 300.0, 582.0], LinkTo::Uri("https://sekka.computer/".into())));
    let second = Page::letter().text(72.0, 720.0, 18.0, "Page two: a needle, and a Needle.");
    let mut third = Page { size: (792.0, 612.0), ..Page::letter() }.text(72.0, 540.0, 18.0, "Page three is wide.");
    third.boxes.push((72.0, 72.0, 200.0, 100.0, 0.2));
    let mut fourth = Page::letter().text(72.0, 720.0, 18.0, "A turned page with a needle.");
    fourth.rotate = 90;
    pdf(
        &[first, second, third, fourth],
        &[Mark("Introduction", 0, vec![]), Mark("Wide", 2, vec![Mark("Turned", 3, vec![])])],
    )
}

pub fn write(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
    let p = dir.join(name);
    std::fs::write(&p, bytes).unwrap();
    p
}

/// A long document: `n` letter pages, each saying its number.
pub fn long(n: usize) -> Vec<u8> {
    let pages: Vec<Page> =
        (0..n).map(|i| Page::letter().text(72.0, 720.0, 18.0, &format!("This is page {} of {n}.", i + 1))).collect();
    pdf(&pages, &[])
}
