//! Builds sekka.computer: static HTML and CSS from the brand tokens, logos
//! and screenshots in this repository. The only JavaScript browsers get is
//! theme.js, which remembers the bar's Night/Day switch. The one
//! piece of server-side code, the download worker (site/download-worker),
//! is built apart and runs for /download/ alone (_routes.json).

mod brand;
mod color;
mod glyphs;
mod html;
mod pages;
mod shared;
mod tokens;

use anyhow::{Context, Result, bail};
use std::collections::{BTreeMap, HashMap};
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
    /// Published path ("images/yukimi.png") to the URL it is served at.
    assets: BTreeMap<String, String>,
}

/// Screenshots shown on the site, copied byte for byte. Their provenance is
/// recorded in docs/screenshots.md, and every caption says it.
const IMAGES: [(&str, &str); 14] = [
    ("aizome-desktop.png", "docs/images/sekka/aizome-desktop.png"),
    ("aizome-menu.png", "docs/images/sekka/aizome-menu.png"),
    ("aizome-palette.png", "docs/images/sekka/aizome-palette.png"),
    ("aizome-settings.png", "docs/images/sekka/aizome-settings.png"),
    ("aizome-yukimi.png", "docs/images/sekka/aizome-yukimi.png"),
    ("genkan.png", "docs/images/sekka/genkan.png"),
    ("yukimi.png", "docs/images/sekka/yukimi.png"),
    ("tatami-menu.png", "docs/images/sekka/tatami-menu.png"),
    ("displays.png", "docs/images/sekka/displays.png"),
    ("plasma.png", "docs/images/sekka/plasma.png"),
    ("gnome.png", "docs/images/sekka/gnome.png"),
    ("network.png", "docs/images/sekka/network.png"),
    ("activity.png", "docs/images/sekka/activity.png"),
    (
        "activity-memory.png",
        "docs/images/sekka/activity-memory.png",
    ),
];

/// The Sekka × Aizome video on YouTube (github.com/bitemyapp/sekka-aizome-hype).
const VIDEO: &str = "jF4OwL1GbxY";
/// Where its player comes from, the one origin the security policy lets a
/// frame load (shared.rs).
const PLAYER: &str = "https://www.youtube-nocookie.com";
/// What the home page shows in the video's place until it is played: the
/// video's frame 1284 (21.4 s), rendered by its `frame` command.
const VIDEO_POSTER: (&str, &str) = ("hype-video.png", "docs/images/sekka/hype-video.png");

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
        "desktop/aizome/icons/apps/org.aizome.Yukimi.svg",
    ),
];

/// Aizome's app icons, as the desktop installs them, published as
/// "apps/<Name>.svg". Toolchest is Aizome's mark on its tile.
pub const APP_ICONS: [(&str, &str); 19] = [
    ("Toolchest", "desktop/aizome/icons/apps/org.aizome.Toolchest.svg"),
    ("Settei", "desktop/aizome/icons/apps/org.aizome.Settei.svg"),
    ("Yukimi", "desktop/aizome/icons/apps/org.aizome.Yukimi.svg"),
    ("Hako", "desktop/aizome/icons/apps/org.aizome.Hako.svg"),
    ("Sumi", "desktop/aizome/icons/apps/org.aizome.Sumi.svg"),
    ("Fusen", "desktop/aizome/icons/apps/org.aizome.Fusen.svg"),
    ("Koyomi", "desktop/aizome/icons/apps/org.aizome.Koyomi.svg"),
    ("Tokei", "desktop/aizome/icons/apps/org.aizome.Tokei.svg"),
    ("Soroban", "desktop/aizome/icons/apps/org.aizome.Soroban.svg"),
    ("Tenki", "desktop/aizome/icons/apps/org.aizome.Tenki.svg"),
    ("Tansu", "desktop/aizome/icons/apps/org.aizome.Tansu.svg"),
    ("Kura", "desktop/aizome/icons/apps/org.aizome.Kura.svg"),
    ("Myaku", "desktop/aizome/icons/apps/org.aizome.Myaku.svg"),
    ("Nisshi", "desktop/aizome/icons/apps/org.aizome.Nisshi.svg"),
    ("Shashin", "desktop/aizome/icons/apps/org.aizome.Shashin.svg"),
    ("Shiori", "desktop/aizome/icons/apps/org.aizome.Shiori.svg"),
    ("Utsushi", "desktop/aizome/icons/apps/org.aizome.Utsushi.svg"),
    ("Koe", "desktop/aizome/icons/apps/org.aizome.Koe.svg"),
    ("Hotaru", "desktop/aizome/apps/hotaru/data/org.aizome.Hotaru.svg"),
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
    /// The URL a file is served at, with a hash of its bytes in the name.
    pub fn asset(&self, path: &str) -> Result<&str> {
        self.assets
            .get(path)
            .map(String::as_str)
            .with_context(|| format!("{path} is not a published file"))
    }

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
        let url = self.asset(&format!("images/{file}"))?;
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
            r#"<figure class="{class}"><a href="{url}" aria-label="{label}"><img src="{url}" width="{w}" height="{h}" alt="{alt}" {loading} decoding="async"></a><figcaption>{caption} <span class="dims">{w} × {h}, not upscaled</span></figcaption></figure>"#,
            label = html::esc(&format!("{alt} Open the full-resolution original.")),
            alt = html::esc(alt),
            caption = html::esc(caption),
        ))
    }

    /// The video in a frame that first holds a page of this site's own: its
    /// poster and a play button, with no script. Nothing is fetched from
    /// YouTube until the button, a link, loads YouTube's player in the frame.
    /// The link sends this site's origin, which the player requires, past the
    /// policy of sending no referrer. The frame has no `src`, so `allow` names
    /// the player's origin: by default it would delegate to the poster's.
    pub fn video(&self, title: &str, caption: &str, class: &str) -> Result<String> {
        let (name, source) = VIDEO_POSTER;
        let (w, h) = png_size(&self.repo.join(source))?;
        let poster = format!(
            r#"<!doctype html><html lang="en"><head><meta name="color-scheme" content="dark"><link rel="stylesheet" href="{css}"></head><body class="poster"><a class="poster-play" href="{PLAYER}/embed/{VIDEO}?autoplay=1&amp;rel=0" referrerpolicy="origin"><img src="{img}" width="{w}" height="{h}" alt=""><span class="button poster-button"><svg class="poster-icon" viewBox="0 0 12 12" aria-hidden="true"><path d="M2 1v10l9-5z" fill="currentColor"/></svg>Play<span class="visually-hidden"> {title}, from YouTube</span></span></a></body></html>"#,
            css = self.asset("site.css")?,
            img = self.asset(&format!("images/{name}"))?,
            title = html::esc(title),
        );
        Ok(format!(
            r#"<figure class="win shot video {class}"><iframe srcdoc="{poster}" title="{title}" width="{w}" height="{h}" allow="autoplay {PLAYER}; encrypted-media {PLAYER}; fullscreen {PLAYER}; picture-in-picture {PLAYER}"></iframe><figcaption>{caption} <a href="https://www.youtube.com/watch?v={VIDEO}">Watch on YouTube</a></figcaption></figure>"#,
            poster = html::esc(&poster),
            title = html::esc(title),
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

fn write(out: &Path, rel: &str, data: impl AsRef<[u8]>) -> Result<()> {
    let path = out.join(rel);
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    fs::write(&path, data).with_context(|| format!("writing {}", path.display()))
}

/// 64-bit FNV-1a. Each step is a bijection of the state, so any one changed
/// run of bytes always changes the result.
fn content_hash(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |h, &b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// "images/yukimi.png" becomes "images/yukimi.<hash>.png".
fn hashed_name(path: &str, bytes: &[u8]) -> Result<String> {
    let (stem, ext) = path
        .rsplit_once('.')
        .with_context(|| format!("{path} has no extension"))?;
    Ok(format!("{stem}.{:016x}.{ext}", content_hash(bytes)))
}

/// Writes a file under its hashed name. A changed file gets a new URL, so
/// these can be cached for a year while pages still revalidate: a deployment
/// never serves an old image beside a new page, and needs no cache purge.
fn publish(
    out: &Path,
    path: &str,
    bytes: &[u8],
    assets: &mut BTreeMap<String, String>,
) -> Result<String> {
    let name = hashed_name(path, bytes)?;
    write(out, &name, bytes)?;
    assets.insert(path.to_owned(), format!("/{name}"));
    Ok(name)
}

fn publish_file(
    from: &Path,
    out: &Path,
    path: &str,
    assets: &mut BTreeMap<String, String>,
) -> Result<()> {
    let bytes = fs::read(from).with_context(|| format!("reading {}", from.display()))?;
    publish(out, path, &bytes, assets).map(drop)
}

/// Every local link, image and font URL in the pages and stylesheet must be a
/// file this run wrote, so a renamed file can't leave a reference dangling.
fn check_links(out: &Path, written: &[(String, String)]) -> Result<()> {
    let mut missing = Vec::new();
    for (file, text) in written {
        for opener in ["href=\"", "src=\"", "url(\""] {
            for part in text.split(opener).skip(1) {
                let Some(url) = part.split('"').next() else {
                    continue;
                };
                if !url.starts_with('/') || url.starts_with("//") {
                    continue;
                }
                let path = url.split(['#', '?']).next().unwrap_or(url);
                let target = if path.ends_with('/') {
                    out.join(&path[1..]).join("index.html")
                } else {
                    out.join(&path[1..])
                };
                if !target.is_file() {
                    missing.push(format!("{file}: {url}"));
                }
            }
        }
    }
    if !missing.is_empty() {
        bail!(
            "links to files that were not written:\n{}",
            missing.join("\n")
        );
    }
    Ok(())
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

/// Cloudflare Pages reads at most this many rules from _headers.
const HEADER_RULES: usize = 100;

fn main() -> Result<()> {
    let a = args()?;
    let out = &a.out;
    fs::create_dir_all(out)?;

    let mut assets = BTreeMap::new();
    for (name, source) in IMAGES.into_iter().chain([VIDEO_POSTER]) {
        publish_file(
            &a.repo.join(source),
            out,
            &format!("images/{name}"),
            &mut assets,
        )?;
    }
    for (name, source) in BRAND_FILES {
        publish_file(
            &a.repo.join(source),
            out,
            &format!("brand/{name}"),
            &mut assets,
        )?;
    }
    for (name, source) in APP_ICONS {
        publish_file(
            &a.repo.join(source),
            out,
            &format!("apps/{name}.svg"),
            &mut assets,
        )?;
    }
    for (from, path) in [
        (&a.mono, "fonts/JetBrainsMono.ttf"),
        (&a.mono_license, "fonts/JetBrainsMono-OFL.txt"),
        (&a.sans, "fonts/AtkinsonHyperlegibleNext.ttf"),
        (&a.sans_italic, "fonts/AtkinsonHyperlegibleNext-Italic.ttf"),
        (&a.sans_license, "fonts/AtkinsonHyperlegibleNext-OFL.txt"),
    ] {
        publish_file(from, out, path, &mut assets)?;
    }

    let tokens = tokens::Tokens::load(&a.repo.join("docs/brand/tokens.json"))?;
    let mut site = Site {
        brand: brand::Brand::new(&a.repo),
        kanji: glyphs::Kanji::load(&a.cjk, 600.0)?,
        revision: a.revision.clone(),
        date: a.date.clone(),
        images: IMAGES.into_iter().collect(),
        repo: a.repo.clone(),
        assets,
        tokens,
    };
    let mut written = Vec::new();

    let mut css = include_str!("../style.css").to_owned();
    // The stylesheet names the fonts by their plain paths.
    for (path, url) in &site.assets {
        css = css.replace(&format!("\"/{path}\""), &format!("\"{url}\""));
    }
    css.push('\n');
    css.push_str(&site.tokens.css()?);
    css.push_str(&swatch_css(&site.tokens)?);
    // Hashed too: Cloudflare otherwise lets browsers keep a stylesheet for
    // hours, which could pair new pages with old styles.
    let name = publish(out, "site.css", css.as_bytes(), &mut site.assets)?;
    written.push((name, css));
    // The bar's Night/Day switch, the one script browsers get, hashed as
    // the stylesheet is.
    let script = include_str!("../theme.js");
    let name = publish(out, "theme.js", script.as_bytes(), &mut site.assets)?;
    written.push((name, script.to_owned()));

    let pages = [
        pages::home(&site)?,
        pages::aizome(&site)?,
        pages::tatami(&site)?,
        pages::yukimi(&site)?,
        pages::design(&site)?,
        pages::status(&site)?,
        pages::download(&site)?,
        pages::fonts(&site)?,
    ];
    let mut sitemap = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<urlset xmlns=\"http://www.sitemaps.org/schemas/sitemap/0.9\">\n",
    );
    for page in &pages {
        let file = format!("{}index.html", &page.path[1..]);
        let html = html::shell(&site, page)?;
        write(out, &file, &html)?;
        written.push((file, html));
        writeln!(
            sitemap,
            "  <url><loc>{}{}</loc></url>",
            html::ORIGIN,
            page.path
        )?;
    }
    sitemap.push_str("</urlset>\n");
    let lost = pages::not_found(&site)?;
    let html = html::shell(&site, &lost)?;
    write(out, "404.html", &html)?;
    written.push(("404.html".to_owned(), html));
    write(out, "sitemap.xml", &sitemap)?;
    write(
        out,
        "robots.txt",
        format!(
            "User-agent: *\nAllow: /\nSitemap: {}/sitemap.xml\n",
            html::ORIGIN
        ),
    )?;

    // Hashed files never change under their names. Everything else, pages
    // included, keeps Pages' default and is revalidated on every visit.
    let mut headers = String::from("/*\n");
    for (name, value) in shared::HEADERS {
        writeln!(headers, "  {name}: {value}")?;
    }
    for url in site.assets.values() {
        writeln!(
            headers,
            "{url}\n  Cache-Control: public, max-age=31536000, immutable"
        )?;
    }
    if site.assets.len() + 1 > HEADER_RULES {
        bail!("_headers would have more than {HEADER_RULES} rules");
    }
    write(out, "_headers", headers)?;
    write(
        out,
        "_routes.json",
        r#"{"version": 1, "include": ["/download", "/download/*"], "exclude": []}
"#,
    )?;

    // Pages link the hashed app icon; this keeps the usual address working.
    copy(
        &a.repo.join("docs/brand/logos/sekka-app-icon.svg"),
        out,
        "favicon.svg",
    )?;
    check_links(out, &written)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashed_names_keep_the_directory_and_extension() {
        let name = hashed_name("images/yukimi.png", b"snow").unwrap();
        assert!(name.starts_with("images/yukimi."), "{name}");
        assert!(name.ends_with(".png"), "{name}");
        assert_eq!(name.len(), "images/yukimi..png".len() + 16);
        assert_eq!(name, hashed_name("images/yukimi.png", b"snow").unwrap());
    }

    #[test]
    fn dangling_links_are_caught() {
        let out = std::env::temp_dir().join(format!("sekka-site-links-{}", std::process::id()));
        fs::create_dir_all(out.join("tatami")).unwrap();
        fs::write(out.join("tatami/index.html"), "").unwrap();
        fs::write(out.join("site.css"), "").unwrap();
        let page = |html: &str| vec![("index.html".to_owned(), html.to_owned())];
        let good = check_links(
            &out,
            &page(
                r##"<a href="/tatami/#keys">t</a><link href="/site.css"><a href="https://example.com/">e</a><a href="#main">m</a>"##,
            ),
        );
        let bad = check_links(&out, &page(r#"<img src="/images/yukimi.png">"#));
        fs::remove_dir_all(&out).unwrap();
        good.unwrap();
        assert!(
            bad.unwrap_err()
                .to_string()
                .contains("index.html: /images/yukimi.png")
        );
    }

    #[test]
    fn any_changed_byte_changes_the_name() {
        let base = vec![0u8; 64];
        let first = hashed_name("a.png", &base).unwrap();
        for i in 0..base.len() {
            let mut changed = base.clone();
            changed[i] = 1;
            assert_ne!(hashed_name("a.png", &changed).unwrap(), first, "byte {i}");
        }
    }
}
