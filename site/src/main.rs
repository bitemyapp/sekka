//! Builds sekka.computer: static HTML and CSS from the brand tokens, logos
//! and screenshots in this repository. No JavaScript is emitted.

mod brand;
mod color;
mod glyphs;
mod html;
mod pages;
mod tokens;

use anyhow::{Context, Result, bail};
use std::collections::HashMap;
use std::fmt::Write;
use std::fs;
use std::path::{Path, PathBuf};

const USAGE: &str = "Usage: sekka-site --repo DIR --out DIR --revision REV --date YYYY-MM-DD \
--mono TTF --mono-license TXT --sans TTF --sans-italic TTF --sans-license TXT --cjk TTC";

pub struct Site {
    pub tokens: tokens::Tokens,
    pub brand: brand::Brand,
    pub kanji: glyphs::Kanji,
    pub revision: String,
    pub date: String,
    repo: PathBuf,
    images: HashMap<&'static str, &'static str>,
}

/// Screenshots shown on the site, copied byte for byte. Their provenance is
/// recorded in docs/screenshots.md, and every caption says it.
const IMAGES: [(&str, &str); 6] = [
    ("genkan.png", "docs/images/sekka/genkan.png"),
    ("yukimi.png", "docs/images/sekka/yukimi.png"),
    ("tatami.png", "docs/images/sekka/tatami.png"),
    ("tatami-menu.png", "docs/images/sekka/tatami-menu.png"),
    ("plasma.png", "docs/images/sekka/plasma.png"),
    ("gnome.png", "docs/images/sekka/gnome.png"),
];

const BRAND_FILES: [(&str, &str); 8] = [
    (
        "sekka-lockup-night.svg",
        "docs/brand/logos/sekka-lockup-night.svg",
    ),
    (
        "sekka-lockup-day.svg",
        "docs/brand/logos/sekka-lockup-day.svg",
    ),
    (
        "sekka-crest-blue.svg",
        "docs/brand/logos/sekka-crest-blue.svg",
    ),
    ("sekka-symbolic.svg", "docs/brand/logos/sekka-symbolic.svg"),
    ("sekka-app-icon.svg", "docs/brand/logos/sekka-app-icon.svg"),
    ("genkan.svg", "docs/brand/family/genkan.svg"),
    ("tatami.svg", "docs/brand/family/tatami.svg"),
    (
        "yukimi.svg",
        "apps/yukimi/yukimi/data/io.github.bitemyapp.Yukimi.svg",
    ),
];

/// A PNG's pixel size, from its header, so images reserve their space.
fn png_size(path: &Path) -> Result<(u32, u32)> {
    let bytes = fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    if bytes.len() < 24 || &bytes[..8] != b"\x89PNG\r\n\x1a\n" {
        bail!("{} is not a PNG", path.display());
    }
    let be = |i: usize| u32::from_be_bytes([bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]]);
    Ok((be(16), be(20)))
}

impl Site {
    /// Kanji outlined, with the text kept for screen readers and copying.
    pub fn ja(&self, text: &str) -> Result<String> {
        Ok(format!(
            r#"<span class="ja" lang="ja">{}<span class="visually-hidden">{}</span></span>"#,
            self.kanji.svg(text)?,
            html::esc(text)
        ))
    }

    /// The revision the site was built from, linked only when it is a commit
    /// GitHub can show: a dirty or unknown tree has no page there.
    pub fn revision_html(&self) -> String {
        let rev = html::esc(&self.revision);
        if self.revision.chars().all(|c| c.is_ascii_hexdigit()) && !self.revision.is_empty() {
            format!(
                r#"<a href="{}/commit/{rev}"><code>{rev}</code></a>"#,
                html::REPO
            )
        } else {
            format!("<code>{rev}</code>")
        }
    }

    /// A screenshot at its own size, linked to the full-resolution original.
    pub fn shot(&self, file: &str, alt: &str, caption: &str, class: &str) -> Result<String> {
        let source = self
            .images
            .get(file)
            .with_context(|| format!("{file} is not a listed screenshot"))?;
        let (w, h) = png_size(&self.repo.join(source))?;
        // The first screenshot on a page is in view at once: fetch it first.
        let loading = if class.contains("hero-shot") || class.contains("wide-shot") {
            r#"loading="eager" fetchpriority="high""#
        } else {
            r#"loading="lazy""#
        };
        let class = if class.is_empty() {
            String::from("shot")
        } else {
            format!("shot {class}")
        };
        Ok(format!(
            r#"<figure class="{class}"><a href="/images/{file}" aria-label="{label}"><img src="/images/{file}" width="{w}" height="{h}" alt="{alt}" {loading} decoding="async"></a><figcaption>{caption} <span class="dims">{w} × {h}, not upscaled</span></figcaption></figure>"#,
            label = html::esc(&format!("{alt} Open the full-resolution original.")),
            alt = html::esc(alt),
            caption = html::esc(caption),
        ))
    }
}

struct Args {
    repo: PathBuf,
    out: PathBuf,
    revision: String,
    date: String,
    mono: PathBuf,
    mono_license: PathBuf,
    sans: PathBuf,
    sans_italic: PathBuf,
    sans_license: PathBuf,
    cjk: PathBuf,
}

fn args() -> Result<Args> {
    let mut map = HashMap::new();
    let mut it = std::env::args().skip(1);
    while let Some(flag) = it.next() {
        let key = flag
            .strip_prefix("--")
            .with_context(|| format!("unexpected {flag}\n{USAGE}"))?
            .to_owned();
        let value = it
            .next()
            .with_context(|| format!("{flag} needs a value\n{USAGE}"))?;
        map.insert(key, value);
    }
    let mut take = |k: &str| {
        map.remove(k)
            .with_context(|| format!("missing --{k}\n{USAGE}"))
    };
    Ok(Args {
        repo: take("repo")?.into(),
        out: take("out")?.into(),
        revision: take("revision")?,
        date: take("date")?,
        mono: take("mono")?.into(),
        mono_license: take("mono-license")?.into(),
        sans: take("sans")?.into(),
        sans_italic: take("sans-italic")?.into(),
        sans_license: take("sans-license")?.into(),
        cjk: take("cjk")?.into(),
    })
}

fn write(out: &Path, rel: &str, text: &str) -> Result<()> {
    let path = out.join(rel);
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    fs::write(&path, text).with_context(|| format!("writing {}", path.display()))
}

fn copy(from: &Path, out: &Path, rel: &str) -> Result<()> {
    let to = out.join(rel);
    if let Some(dir) = to.parent() {
        fs::create_dir_all(dir)?;
    }
    fs::copy(from, &to).with_context(|| format!("copying {}", from.display()))?;
    Ok(())
}

/// Swatch colours as classes: the security policy allows no inline styles.
fn swatch_css(t: &tokens::Tokens) -> Result<String> {
    let mut s = String::new();
    for c in &t.colors {
        for th in &t.themes {
            writeln!(
                s,
                ".sw-{}-{} {{ background: {}; }}",
                c.name,
                th.id,
                t.hex(&c.name, &th.id)?
            )?;
        }
    }
    Ok(s)
}

const HEADERS: &str = "/*
  Content-Security-Policy: default-src 'none'; style-src 'self'; img-src 'self'; font-src 'self'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'
  Referrer-Policy: no-referrer
  X-Content-Type-Options: nosniff
  Permissions-Policy: camera=(), microphone=(), geolocation=(), browsing-topics=()
  Cross-Origin-Opener-Policy: same-origin
  Strict-Transport-Security: max-age=31536000
/fonts/*
  Cache-Control: public, max-age=604800
/images/*
  Cache-Control: public, max-age=604800
/brand/*
  Cache-Control: public, max-age=86400
";

fn main() -> Result<()> {
    let a = args()?;
    let tokens = tokens::Tokens::load(&a.repo.join("docs/brand/tokens.json"))?;
    let site = Site {
        brand: brand::Brand::new(&a.repo),
        kanji: glyphs::Kanji::load(&a.cjk, 600.0)?,
        revision: a.revision.clone(),
        date: a.date.clone(),
        images: IMAGES.into_iter().collect(),
        repo: a.repo.clone(),
        tokens,
    };
    let out = &a.out;
    fs::create_dir_all(out)?;

    let mut css = include_str!("../style.css").to_owned();
    css.push('\n');
    css.push_str(&site.tokens.css()?);
    css.push_str(&swatch_css(&site.tokens)?);
    write(out, "site.css", &css)?;

    let pages = [
        pages::home(&site)?,
        pages::tatami(&site)?,
        pages::yukimi(&site)?,
        pages::design(&site)?,
        pages::status(&site)?,
        pages::fonts(),
    ];
    let mut sitemap = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<urlset xmlns=\"http://www.sitemaps.org/schemas/sitemap/0.9\">\n",
    );
    for page in &pages {
        write(
            out,
            &format!("{}index.html", &page.path[1..]),
            &html::shell(&site, page)?,
        )?;
        writeln!(
            sitemap,
            "  <url><loc>{}{}</loc></url>",
            html::ORIGIN,
            page.path
        )?;
    }
    sitemap.push_str("</urlset>\n");
    let lost = pages::not_found(&site)?;
    write(out, "404.html", &html::shell(&site, &lost)?)?;
    write(out, "sitemap.xml", &sitemap)?;
    write(
        out,
        "robots.txt",
        &format!(
            "User-agent: *\nAllow: /\nSitemap: {}/sitemap.xml\n",
            html::ORIGIN
        ),
    )?;
    write(out, "_headers", HEADERS)?;

    for (name, source) in IMAGES {
        copy(&a.repo.join(source), out, &format!("images/{name}"))?;
    }
    for (name, source) in BRAND_FILES {
        copy(&a.repo.join(source), out, &format!("brand/{name}"))?;
    }
    copy(
        &a.repo.join("docs/brand/logos/sekka-app-icon.svg"),
        out,
        "favicon.svg",
    )?;
    copy(&a.mono, out, "fonts/JetBrainsMono.ttf")?;
    copy(&a.mono_license, out, "fonts/JetBrainsMono-OFL.txt")?;
    copy(&a.sans, out, "fonts/AtkinsonHyperlegibleNext.ttf")?;
    copy(
        &a.sans_italic,
        out,
        "fonts/AtkinsonHyperlegibleNext-Italic.ttf",
    )?;
    copy(
        &a.sans_license,
        out,
        "fonts/AtkinsonHyperlegibleNext-OFL.txt",
    )?;
    Ok(())
}
