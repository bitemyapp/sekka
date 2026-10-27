//! Turns the Lucide icons in `icons/lucide/*.svg` into the `Icon` enum: one
//! variant per file, named after it (`wifi-off.svg` is `Icon::WifiOff`),
//! with the SVG's shape elements kept as text and parsed into kurbo shapes
//! the first time an icon draws (see `src/icons.rs`).

use std::fmt::Write as _;
use std::path::Path;

fn main() {
    let dir = Path::new("icons/lucide");
    println!("cargo:rerun-if-changed={}", dir.display());
    let mut icons: Vec<(String, Vec<String>)> = Vec::new();
    for entry in std::fs::read_dir(dir).expect("icons/lucide") {
        let path = entry.expect("an entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("svg") {
            continue;
        }
        let name = path.file_stem().and_then(|s| s.to_str()).expect("a name").to_owned();
        let svg = std::fs::read_to_string(&path).expect("an SVG");
        icons.push((name, elements(&svg)));
    }
    icons.sort();
    let mut out = String::new();
    out.push_str("/// An icon from Lucide (ISC licence; see `icons/lucide/LICENSE`).\n");
    out.push_str(
        "#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]\n#[non_exhaustive]\npub enum Icon {\n",
    );
    for (name, _) in &icons {
        writeln!(out, "    /// Lucide's `{name}`.\n    {},", camel(name)).unwrap();
    }
    out.push_str("}\n\nimpl Icon {\n    /// Every icon, by name.\n    pub const ALL: &'static [Icon] = &[\n");
    for (name, _) in &icons {
        writeln!(out, "        Icon::{},", camel(name)).unwrap();
    }
    out.push_str("    ];\n\n    /// Lucide's name for it (`wifi-off`).\n    pub fn name(self) -> &'static str {\n        match self {\n");
    for (name, _) in &icons {
        writeln!(out, "            Icon::{} => {name:?},", camel(name)).unwrap();
    }
    out.push_str("        }\n    }\n\n    /// The SVG elements it's drawn from, on Lucide's 24-unit grid.\n    pub(crate) fn elements(self) -> &'static [&'static str] {\n        match self {\n");
    for (name, els) in &icons {
        writeln!(out, "            Icon::{} => &{:?},", camel(name), els).unwrap();
    }
    out.push_str("        }\n    }\n}\n");
    let dest = Path::new(&std::env::var("OUT_DIR").expect("OUT_DIR")).join("icons.rs");
    std::fs::write(dest, out).expect("written");
}

/// The shape elements of an SVG, each as its own `<tag … />` text.
fn elements(svg: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = svg;
    while let Some(start) = rest.find('<') {
        let after = &rest[start + 1..];
        let end = after.find('>').unwrap_or(after.len());
        let tag = &after[..end];
        let name: String = tag.chars().take_while(|c| c.is_ascii_alphabetic()).collect();
        if matches!(name.as_str(), "path" | "circle" | "rect" | "line" | "polyline" | "polygon" | "ellipse") {
            let body = tag.trim_end_matches('/').trim();
            out.push(body.split_whitespace().collect::<Vec<_>>().join(" "));
        }
        rest = &after[end.min(after.len())..];
    }
    out
}

fn camel(name: &str) -> String {
    name.split('-')
        .map(|part| {
            let mut c = part.chars();
            match c.next() {
                Some(f) => f.to_ascii_uppercase().to_string() + c.as_str(),
                None => String::new(),
            }
        })
        .collect()
}
