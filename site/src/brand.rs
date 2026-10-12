//! The brand's SVGs, inlined so they follow the page's theme, and the
//! crest's construction drawn from the same numbers as docs/brand/generate.py.

use anyhow::{Context, Result};
use std::cell::Cell;
use std::f64::consts::PI;
use std::fmt::Write;
use std::path::{Path, PathBuf};

// Night inks only: Day files are not inlined, the classes follow the theme.

pub struct Brand {
    logos: PathBuf,
    next_id: Cell<u32>,
}

/// The fixed inks generate.py writes, and the token classes that replace them.
const INKS: [(&str, &str); 4] = [
    ("fill=\"#7aa2f7\"", "class=\"ink-crest\""),
    ("fill=\"#c0caf5\"", "class=\"ink-strong\""),
    ("fill=\"#a9b1d6\"", "class=\"ink-text\""),
    ("fill=\"#2e3436\"", "class=\"ink-current\""),
];

impl Brand {
    pub fn new(repo: &Path) -> Self {
        Brand {
            logos: repo.join("docs/brand/logos"),
            next_id: Cell::new(0),
        }
    }

    /// A Night logo file, inline, inked by tokens: `crest`, `text-strong`, `text`.
    pub fn inline(&self, file: &str, class: &str, label: &str) -> Result<String> {
        let path = self.logos.join(file);
        let mut svg = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        for (from, to) in INKS {
            svg = svg.replace(from, to);
        }
        // Each inline copy needs its own mask ids.
        let n = self.next_id.get();
        self.next_id.set(n + 1);
        let mut out = String::new();
        let mut rest = svg.as_str();
        while let Some(i) = rest.find("id=\"") {
            let (head, tail) = rest.split_at(i + 4);
            let end = tail.find('"').context("unterminated id")?;
            let id = &tail[..end];
            out.push_str(head);
            write!(out, "{id}-{n}")?;
            rest = &tail[end..];
        }
        out.push_str(rest);
        let mut svg = out.replace("url(#", "url(#\u{0}");
        // url(#x) -> url(#x-n), matching the renamed ids.
        while let Some(i) = svg.find('\u{0}') {
            let end = svg[i..].find(')').context("unterminated url()")? + i;
            let id = svg[i + 1..end].to_owned();
            svg.replace_range(i..end, &format!("{id}-{n}"));
        }
        // The outer element: drop the fixed size, take a class and a label.
        let open = svg.find("<svg").context("not an SVG")?;
        let close = svg[open..].find('>').context("unterminated <svg>")? + open;
        let tag = &svg[open..close];
        let mut new_tag = String::from("<svg");
        for attr in ["xmlns", "viewBox"] {
            if let Some(v) = attribute(tag, attr) {
                write!(new_tag, " {attr}=\"{v}\"")?;
            }
        }
        write!(
            new_tag,
            " class=\"{class}\" role=\"img\" aria-label=\"{label}\""
        )?;
        let mut body = svg[close..].to_owned();
        // The file's own <title> repeats the label; keep one accessible name.
        if let (Some(a), Some(b)) = (body.find("<title>"), body.find("</title>")) {
            body.replace_range(a..b + 8, "");
        }
        if let (Some(a), Some(b)) = (body.find("<desc>"), body.find("</desc>")) {
            body.replace_range(a..b + 7, "");
        }
        Ok(format!("{new_tag}{body}"))
    }
}

fn attribute<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let key = format!(" {name}=\"");
    let start = tag.find(&key)? + key.len();
    let end = tag[start..].find('"')? + start;
    Some(&tag[start..end])
}

// The crest's geometry, as in docs/brand/generate.py.
const C: f64 = 128.0;
const LOBE_D: f64 = 36.0;
const LOBE_R: f64 = 76.0;

fn pt(r: f64, deg: f64) -> (f64, f64) {
    let a = deg * PI / 180.0;
    (C + r * a.cos(), C - r * a.sin())
}

fn cusp_radius() -> f64 {
    let b = 2.0 * LOBE_D * (30.0 * PI / 180.0).cos();
    (b + (b * b - 4.0 * (LOBE_D * LOBE_D - LOBE_R * LOBE_R)).sqrt()) / 2.0
}

fn hexafoil() -> String {
    let t = cusp_radius();
    let cusps: Vec<f64> = (0..6).map(|k| 120.0 + 60.0 * k as f64).collect();
    let (x, y) = pt(t, cusps[5]);
    let mut d = format!("M{x:.2} {y:.2}");
    for c in &cusps {
        let (x, y) = pt(t, *c);
        let _ = write!(d, "A{LOBE_R} {LOBE_R} 0 0 0 {x:.2} {y:.2}");
    }
    d + "Z"
}

fn hexagon(r: f64) -> String {
    let pts: Vec<String> = (0..6)
        .map(|k| {
            let (x, y) = pt(r, 90.0 + 60.0 * k as f64);
            format!("{x:.2} {y:.2}")
        })
        .collect();
    format!("M{}Z", pts.join("L"))
}

fn crystal() -> String {
    let mut d = String::new();
    for k in 0..6 {
        let a = 90.0 + 60.0 * k as f64;
        let (x0, y0) = pt(19.0, a);
        let (x1, y1) = pt(84.0, a);
        let _ = write!(d, "M{x0:.2} {y0:.2}L{x1:.2} {y1:.2}");
        let (bx, by) = pt(60.0, a);
        for side in [60.0, -60.0] {
            let r = (a + side) * PI / 180.0;
            let _ = write!(
                d,
                "M{bx:.2} {by:.2}L{:.2} {:.2}",
                bx + 20.0 * r.cos(),
                by - 20.0 * r.sin()
            );
        }
    }
    d
}

/// The crest taken apart: the six construction circles, the yukiwa they
/// make, and the crystal cut through it.
pub fn construction() -> String {
    let mut s = String::from(
        r#"<svg class="construction" viewBox="-8 -8 272 272" role="img" aria-label="The crest's construction: six circles of radius 76, centred 36 from the middle, make a scalloped ring; a six-armed crystal is cut through it.">"#,
    );
    // The ring first, then the circles that make it, drawn over it.
    let _ = write!(s, r#"<path class="ring" d="{}"/>"#, hexafoil());
    for k in 0..6 {
        let (x, y) = pt(LOBE_D, 90.0 + 60.0 * k as f64);
        let _ = write!(
            s,
            r#"<circle class="guide" cx="{x:.2}" cy="{y:.2}" r="{LOBE_R}"/><circle class="centre" cx="{x:.2}" cy="{y:.2}" r="2.5"/>"#
        );
    }
    let _ = write!(
        s,
        r#"<circle class="guide faint" cx="{C}" cy="{C}" r="{LOBE_D}"/><path class="cut" d="{}"/><path class="core" d="{}"/>"#,
        crystal(),
        hexagon(19.0)
    );
    s.push_str("</svg>");
    s
}
