//! One PDF file through hayro, as the other engines' programs in `../c` do:
//! open it, then draw its first pages (5 unless PDFBENCH_PAGES says, "all"
//! for every page) at PDFBENCH_SCALE (2, i.e. 144 dpi) on white, timing each
//! page from loading it to its finished pixels. Prints one line a step:
//!
//!   open  <ms> <pages>          (or: error <why>)
//!   page  <index> <ms> <width> <height>
//!   done
//!
//! With a directory as the second argument, each page is also written there
//! as `<index>.ppm` (outside the timed part).
//!
//! The document's render cache is kept across its pages, as Shiori's worker
//! keeps it; interpreter and render settings are hayro's defaults (its
//! embedded standard fonts).

use std::io::Write;
use std::time::Instant;

use hayro::hayro_interpret::InterpreterSettings;
use hayro::hayro_syntax::Pdf;
use hayro::vello_cpu::color::palette::css::WHITE;
use hayro::{PixmapSettings, RenderCache, RenderSettings, render};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let Some(path) = args.get(1) else {
        eprintln!("usage: {} FILE.pdf [OUT_DIR]", args[0]);
        std::process::exit(2);
    };
    let out_dir = args.get(2);
    let max_pages = match std::env::var("PDFBENCH_PAGES").as_deref() {
        Ok("all") => usize::MAX,
        Ok(n) => n.parse().unwrap_or(5),
        Err(_) => 5,
    };
    let scale: f32 = std::env::var("PDFBENCH_SCALE").ok().and_then(|s| s.parse().ok()).unwrap_or(2.0);
    let stdout = std::io::stdout();
    let mut out = stdout.lock();

    let t = Instant::now();
    let data = match std::fs::read(path) {
        Ok(d) => d,
        Err(e) => {
            let _ = writeln!(out, "error\tread: {e}");
            return;
        }
    };
    let pdf = match Pdf::new(data) {
        Ok(p) => p,
        Err(e) => {
            let _ = writeln!(out, "error\t{e:?}");
            return;
        }
    };
    let pages = pdf.pages();
    let _ = writeln!(out, "open\t{:.3}\t{}", ms(t), pages.len());
    let _ = out.flush();

    let cache = RenderCache::new();
    let settings = InterpreterSettings::default();
    let render_settings = RenderSettings::default();
    let pixmap_settings = PixmapSettings { x_scale: scale, y_scale: scale, bg_color: WHITE };
    for (i, page) in pages.iter().take(max_pages).enumerate() {
        let t = Instant::now();
        let pixmap = render(page, &cache, &settings, &render_settings, &pixmap_settings);
        let took = ms(t);
        let (w, h) = (pixmap.width(), pixmap.height());
        let _ = writeln!(out, "page\t{i}\t{took:.3}\t{w}\t{h}");
        let _ = out.flush();
        if let Some(dir) = out_dir {
            // Opaque on white, so the premultiplied bytes are straight RGB.
            let rgba = pixmap.data_as_u8_slice();
            let mut ppm = format!("P6\n{w} {h}\n255\n").into_bytes();
            ppm.reserve(rgba.len() / 4 * 3);
            for px in rgba.chunks_exact(4) {
                ppm.extend_from_slice(&px[..3]);
            }
            let _ = std::fs::write(format!("{dir}/{i}.ppm"), ppm);
        }
    }
    let _ = writeln!(out, "done");
}

fn ms(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1000.0
}
