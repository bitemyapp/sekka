//! hayro (the fork or upstream, as the Cargo.toml building it pins) drawing a
//! PDF for PDFium's pixel tests: every
//! page at 72 dpi on white, with the fonts PDFium's tests substitute for fonts
//! a document doesn't embed. pdfium_test runs those tests with
//! `--croscore-font-names` and Chromium's test fonts, so Helvetica (and Arial,
//! Calibri) is drawn with Arimo, Times (and a font with no name) with Tinos,
//! and Courier with Cousine, in their bold and italic faces; this does the
//! same through hayro's font resolver for the standard fonts and for the
//! standard font hayro picks in place of another missing font. Symbol and
//! ZapfDingbats stay hayro's own (as PDFium's built-in ones serve them).
//! Prints as the benchmark (main.rs) does and writes `<index>.ppm`.
//!
//!   pdfbench-hayro-{fork,upstream}-pixel FILE.pdf OUT_DIR [FONT_DIR]

use std::io::Write;
use std::sync::Arc;
use std::time::Instant;

use hayro::hayro_interpret::font::{FontData, FontQuery, StandardFont};
use hayro::hayro_interpret::InterpreterSettings;
use hayro::hayro_syntax::Pdf;
use hayro::vello_cpu::color::palette::css::WHITE;
use hayro::{PixmapSettings, RenderCache, RenderSettings, render};

fn substitute(dir: &str, font: StandardFont) -> Option<FontData> {
    let name = format!("{font:?}");
    let family = if name.contains("Helvetica") {
        "Arimo"
    } else if name.contains("Times") {
        "Tinos"
    } else if name.contains("Courier") {
        "Cousine"
    } else {
        return None;
    };
    let bold = name.contains("Bold");
    let italic = name.contains("Oblique") || name.contains("Italic");
    let face = match (bold, italic) {
        (false, false) => "Regular",
        (true, false) => "Bold",
        (false, true) => "Italic",
        (true, true) => "BoldItalic",
    };
    let data = std::fs::read(format!("{dir}/{family}-{face}.ttf")).ok()?;
    Some(Arc::new(data))
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("usage: {} FILE.pdf OUT_DIR [FONT_DIR]", args[0]);
        std::process::exit(2);
    }
    let (path, out_dir) = (&args[1], &args[2]);
    let mut settings = InterpreterSettings::default();
    if let Some(dir) = args.get(3).cloned() {
        settings.font_resolver = Arc::new(move |query| {
            let standard = match query {
                FontQuery::Standard(s) => *s,
                FontQuery::Fallback(f) => f.pick_standard_font(),
            };
            match substitute(&dir, standard) {
                Some(data) => Some((data, 0)),
                None => Some(standard.get_font_data()),
            }
        });
    }
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    let t = Instant::now();
    let pdf = match std::fs::read(path).map_err(|e| format!("{e}")).and_then(|d| Pdf::new(d).map_err(|e| format!("{e:?}"))) {
        Ok(p) => p,
        Err(e) => {
            let _ = writeln!(out, "error\t{e}");
            return;
        }
    };
    let pages = pdf.pages();
    let _ = writeln!(out, "open\t{:.3}\t{}", t.elapsed().as_secs_f64() * 1000.0, pages.len());
    let cache = RenderCache::new();
    let pixmap_settings = PixmapSettings { x_scale: 1.0, y_scale: 1.0, bg_color: WHITE };
    for (i, page) in pages.iter().enumerate() {
        let t = Instant::now();
        let pixmap = render(page, &cache, &settings, &RenderSettings::default(), &pixmap_settings);
        let (w, h) = (pixmap.width(), pixmap.height());
        let _ = writeln!(out, "page\t{i}\t{:.3}\t{w}\t{h}", t.elapsed().as_secs_f64() * 1000.0);
        let _ = out.flush();
        let mut ppm = format!("P6\n{w} {h}\n255\n").into_bytes();
        for px in pixmap.data_as_u8_slice().chunks_exact(4) {
            ppm.extend_from_slice(&px[..3]);
        }
        let _ = std::fs::write(format!("{out_dir}/{i}.ppm"), ppm);
    }
    let _ = writeln!(out, "done");
}
