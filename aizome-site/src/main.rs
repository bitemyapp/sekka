//! Builds aizome.computer: static HTML and CSS from Aizome's theme, icons,
//! docs and screenshots in this repository. The only JavaScript browsers
//! get is theme.js, which remembers the menu bar's Night/Day switch.

mod apps;
mod color;
mod glyphs;
mod html;
mod pages;
mod theme;

use anyhow::{Context, Result, bail};
use std::collections::{BTreeMap, HashMap};
use std::fmt::Write;
use std::fs;
use std::path::{Path, PathBuf};

const USAGE: &str = "Usage: aizome-site --repo DIR --out DIR --revision REV --date YYYY-MM-DD \
--sans WOFF2 --sans-license TXT --mono WOFF2 --mono-semibold WOFF2 --mono-license TXT \
--cjk TTC --symbols FONT[:FONT…]";

pub struct Site {
    pub theme: theme::Theme,
    pub revision: String,
    pub date: String,
    kanji: glyphs::Outliner,
    symbols: glyphs::Outliner,
    repo: PathBuf,
    images: HashMap<&'static str, &'static str>,
    /// Published path ("images/kura.png") to the URL it is served at.
    assets: BTreeMap<String, String>,
}

/// Screenshots shown on the site, copied byte for byte and never scaled.
/// Each caption repeats the provenance recorded beside the file: in
/// docs/screenshots.md, desktop/aizome/docs/screenshots/README.md, or the
/// doc that shows it (desktop/aizome/docs/ipc.md, recovery.md,
/// docs/test-results/application-startup/README.md).
const IMAGES: [(&str, &str); 27] = [
    ("desktop.png", "docs/images/sekka/aizome-desktop.png"),
    ("menu.png", "docs/images/sekka/aizome-menu.png"),
    ("palette.png", "docs/images/sekka/aizome-palette.png"),
    ("settei.png", "docs/images/sekka/aizome-settings.png"),
    ("yukimi.png", "docs/images/sekka/aizome-yukimi.png"),
    ("genkan.png", "docs/images/sekka/genkan.png"),
    (
        "walkthrough-desktop.png",
        "desktop/aizome/docs/screenshots/01-desktop-night.png",
    ),
    (
        "walkthrough-menus.png",
        "desktop/aizome/docs/screenshots/02-menus.png",
    ),
    (
        "walkthrough-palette.png",
        "desktop/aizome/docs/screenshots/03-palette.png",
    ),
    (
        "overview.png",
        "desktop/aizome/docs/screenshots/04-overview.png",
    ),
    (
        "switcher.png",
        "desktop/aizome/docs/screenshots/05-switcher.png",
    ),
    ("day.png", "desktop/aizome/docs/screenshots/07-day.png"),
    ("kura.png", "desktop/aizome/docs/screenshots/09-kura.png"),
    (
        "tokei-tenki.png",
        "desktop/aizome/docs/screenshots/10-tokei-tenki.png",
    ),
    (
        "marking-menu.png",
        "desktop/aizome/docs/screenshots/11-marking-menu.png",
    ),
    ("lock.png", "desktop/aizome/docs/screenshots/12-lock.png"),
    (
        "full-effects.png",
        "desktop/aizome/docs/screenshots/13-full-effects.png",
    ),
    (
        "recording.png",
        "desktop/aizome/docs/screenshots/14-recording.png",
    ),
    (
        "recording-saved.png",
        "desktop/aizome/docs/screenshots/16-recording-saved.png",
    ),
    ("recovery.png", "desktop/aizome/docs/screenshots/recovery.png"),
    (
        "nisshi-night.png",
        "desktop/aizome/docs/screenshots/nisshi/night-selected.png",
    ),
    (
        "nisshi-day.png",
        "desktop/aizome/docs/screenshots/nisshi/day-search.png",
    ),
    (
        "group-soroban.png",
        "desktop/aizome/docs/images/heterogeneous-group-calculator.png",
    ),
    (
        "group-chrome.png",
        "desktop/aizome/docs/images/heterogeneous-group-browser.png",
    ),
    (
        "group-detached.png",
        "desktop/aizome/docs/images/heterogeneous-group-detached.png",
    ),
    (
        "koe.png",
        "docs/test-results/application-startup/koe-window-live.png",
    ),
    (
        "settei-displays.png",
        "docs/test-results/luks-gui-uefi/75-settings-displays.png",
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

/// The keys Aizome prints as glyphs, and what a screen reader says.
const KEY_GLYPHS: [(char, &str); 12] = [
    ('❖', "Super"),
    ('⇧', "Shift"),
    ('⌥', "Alt"),
    ('⌃', "Control"),
    ('↩', "Return"),
    ('⇥', "Tab"),
    ('⎋', "Escape"),
    ('⌫', "Backspace"),
    ('←', "Left"),
    ('→', "Right"),
    ('↑', "Up"),
    ('↓', "Down"),
];

/// How a screenshot is framed.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Frame {
    /// A window of its own, titled by the file's name as Shashin titles it.
    Window,
    /// The focused window.
    Focus,
    /// A window, the page's first picture: fetched before the rest.
    Lead,
    /// Bare, inside a frame that is already there (a tab group's panel).
    Bare,
    /// Bare, and the page's first picture.
    BareLead,
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
            self.kanji.svg(text, "kanji")?,
            html::esc(text)
        ))
    }

    /// Kanji set one above the other, as the seal beside the wordmark.
    pub fn ja_vertical(&self, text: &str, class: &str) -> Result<String> {
        let mut s = format!(r#"<span class="seal {class}" lang="ja">"#);
        for ch in text.chars() {
            s.push_str(&self.kanji.svg(&ch.to_string(), "kanji")?);
        }
        write!(
            s,
            r#"<span class="visually-hidden">{}</span></span>"#,
            html::esc(text)
        )?;
        Ok(s)
    }

    /// A shortcut as keycaps: "⌃ ❖ ↑" is Control, Super and Up. Glyph keys
    /// are drawn from the symbol fonts and named for screen readers; other
    /// keys are their text.
    pub fn keys(&self, spec: &str) -> Result<String> {
        let mut s = String::from(r#"<kbd class="keys">"#);
        // Modifiers first, in the order Aizome prints them: ⌃ ⌥ ⇧ ❖.
        let rank = |t: &str| ["⌃", "⌥", "⇧", "❖"].iter().position(|m| *m == t);
        let mut tokens: Vec<&str> = spec.split_whitespace().collect();
        tokens.sort_by_key(|t| rank(t).unwrap_or(4));
        for token in tokens {
            let mut chars = token.chars();
            let glyph = match (chars.next(), chars.next()) {
                (Some(c), None) => KEY_GLYPHS.iter().find(|(g, _)| *g == c),
                _ => None,
            };
            match glyph {
                Some((g, name)) => write!(
                    s,
                    r#"<kbd class="key glyph">{}<span class="visually-hidden">{name}</span></kbd>"#,
                    self.symbols.svg_tight(&g.to_string(), "key-glyph")?
                )?,
                None => write!(s, r#"<kbd class="key">{}</kbd>"#, html::esc(token))?,
            }
        }
        s.push_str("</kbd>");
        Ok(s)
    }

    /// Expands the page source's shorthands: ⟦⌃ ❖ ↑⟧ for keycaps and
    /// ⟪藍染⟫ for outlined kanji.
    pub fn render(&self, body: &str) -> Result<String> {
        let mut out = String::with_capacity(body.len() * 2);
        let mut rest = body;
        loop {
            let next = [('⟦', '⟧'), ('⟪', '⟫')]
                .into_iter()
                .filter_map(|(open, close)| rest.find(open).map(|i| (i, open, close)))
                .min_by_key(|(i, _, _)| *i);
            let Some((i, open, close)) = next else {
                out.push_str(rest);
                return Ok(out);
            };
            out.push_str(&rest[..i]);
            let after = &rest[i + open.len_utf8()..];
            let end = after
                .find(close)
                .with_context(|| format!("unclosed {open} near {}", &rest[i..]))?;
            let inner = &after[..end];
            if open == '⟦' {
                out.push_str(&self.keys(inner)?);
            } else {
                out.push_str(&self.ja(inner)?);
            }
            rest = &after[end + close.len_utf8()..];
        }
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

    /// An app's icon, as the desktop installs it.
    pub fn icon(&self, id: &str, class: &str, size: u32) -> Result<String> {
        Ok(format!(
            r#"<img class="{class}" src="{}" width="{size}" height="{size}" alt="">"#,
            self.asset(&format!("icons/{id}.svg"))?
        ))
    }

    /// A screenshot at its own size, linked to the full-resolution original.
    pub fn shot(&self, file: &str, alt: &str, caption: &str, frame: Frame) -> Result<String> {
        let source = self
            .images
            .get(file)
            .with_context(|| format!("{file} is not a listed screenshot"))?;
        let (w, h) = png_size(&self.repo.join(source))?;
        let url = self.asset(&format!("images/{file}"))?;
        // A page's first screenshot is in view at once: fetch it first.
        let loading = if matches!(frame, Frame::Focus | Frame::Lead | Frame::BareLead) {
            r#"loading="eager" fetchpriority="high""#
        } else {
            r#"loading="lazy""#
        };
        let figure = format!(
            r#"<a class="shot-link" href="{url}" aria-label="{label}"><img src="{url}" width="{w}" height="{h}" alt="{alt}" {loading} decoding="async"></a><figcaption>{caption} <span class="dims">{w} × {h}, not upscaled</span></figcaption>"#,
            label = html::esc(&format!("{alt} Open the full-resolution original.")),
            alt = html::esc(alt),
            caption = html::esc(caption),
        );
        let name = source.rsplit('/').next().unwrap_or(source);
        Ok(match frame {
            Frame::Bare | Frame::BareLead => format!(r#"<figure class="shot">{figure}</figure>"#),
            Frame::Window | Frame::Focus | Frame::Lead => format!(
                r#"<figure class="window shot{focus}"><div class="tab" aria-hidden="true">{name}</div><div class="frame">{figure}</div></figure>"#,
                focus = if frame == Frame::Focus { " focus" } else { "" },
                name = html::esc(name),
            ),
        })
    }

    /// Windows stacked into one group, as Aizome stacks them: their tabs side
    /// by side along one top edge, one window showing at a time. Radio
    /// buttons choose, so it works without script, and the arrow keys move
    /// between tabs.
    pub fn group(&self, name: &str, label: &str, windows: &[(&str, String)]) -> Result<String> {
        if windows.is_empty() || windows.len() > 4 {
            bail!("a group on the site holds one to four windows");
        }
        let mut radios = String::new();
        let mut tabs = String::new();
        let mut panels = String::new();
        for (i, (title, body)) in windows.iter().enumerate() {
            let n = i + 1;
            let checked = if n == 1 { " checked" } else { "" };
            write!(
                radios,
                r#"<input type="radio" class="tg tg-{n}" name="{name}" id="{name}-{n}"{checked}>"#
            )?;
            write!(
                tabs,
                r#"<label class="tab" for="{name}-{n}">{}</label>"#,
                html::esc(title)
            )?;
            write!(panels, r#"<div class="panel">{body}</div>"#)?;
        }
        Ok(format!(
            r#"<div class="window focus group" role="group" aria-label="{label}">{radios}<div class="tabs">{tabs}</div><div class="frame panels">{panels}</div></div>"#,
            label = html::esc(label)
        ))
    }
}

/// A window: a Haiku tab only as wide as its title (the section's heading),
/// over a square frame with a hard shadow. `class` adds "focus" for the
/// focused window, or layout classes.
pub fn win(id: &str, title: &str, class: &str, body: &str) -> String {
    format!(
        r#"<section class="window {class}" aria-labelledby="{id}"><h2 class="tab" id="{id}">{title}</h2><div class="frame">{body}</div></section>"#
    )
}

struct Args {
    repo: PathBuf,
    out: PathBuf,
    revision: String,
    date: String,
    sans: PathBuf,
    sans_license: PathBuf,
    mono: PathBuf,
    mono_semibold: PathBuf,
    mono_license: PathBuf,
    cjk: PathBuf,
    symbols: Vec<PathBuf>,
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
        sans: take("sans")?.into(),
        sans_license: take("sans-license")?.into(),
        mono: take("mono")?.into(),
        mono_semibold: take("mono-semibold")?.into(),
        mono_license: take("mono-license")?.into(),
        cjk: take("cjk")?.into(),
        symbols: take("symbols")?.split(':').map(PathBuf::from).collect(),
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

/// "images/kura.png" becomes "images/kura.<hash>.png".
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

/// Every in-page link (#id) must name an element on that page.
fn check_anchors(written: &[(String, String)]) -> Result<()> {
    let mut missing = Vec::new();
    let pages: HashMap<String, &str> = written
        .iter()
        .filter(|(f, _)| f.ends_with(".html"))
        .map(|(f, t)| {
            let path = if f == "index.html" {
                String::from("/")
            } else if let Some(dir) = f.strip_suffix("index.html") {
                format!("/{dir}")
            } else {
                format!("/{f}")
            };
            (path, t.as_str())
        })
        .collect();
    for (path, text) in &pages {
        for part in text.split("href=\"").skip(1) {
            let Some(url) = part.split('"').next() else {
                continue;
            };
            let Some((page, id)) = url.split_once('#') else {
                continue;
            };
            let target = if page.is_empty() {
                Some(*text)
            } else if page.starts_with('/') {
                pages.get(page).copied()
            } else {
                None
            };
            if let Some(target) = target
                && !target.contains(&format!("id=\"{id}\""))
            {
                missing.push(format!("{path}: {url}"));
            }
        }
    }
    if !missing.is_empty() {
        bail!("links to anchors that don't exist:\n{}", missing.join("\n"));
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

/// Cloudflare Pages reads at most this many rules from _headers.
const HEADER_RULES: usize = 100;

fn main() -> Result<()> {
    let a = args()?;
    let out = &a.out;
    fs::create_dir_all(out)?;

    let mut assets = BTreeMap::new();
    for (name, source) in IMAGES {
        publish_file(
            &a.repo.join(source),
            out,
            &format!("images/{name}"),
            &mut assets,
        )?;
    }
    for app in apps::APPS {
        publish_file(
            &a.repo.join(app.icon),
            out,
            &format!("icons/{}.svg", app.id()),
            &mut assets,
        )?;
    }
    publish_file(
        &a.repo.join(apps::TOOLCHEST_ICON),
        out,
        "icons/org.aizome.Toolchest.svg",
        &mut assets,
    )?;
    for (from, path) in [
        (&a.sans, "fonts/Archivo.woff2"),
        (&a.sans_license, "fonts/Archivo-OFL.txt"),
        (&a.mono, "fonts/IBMPlexMono-Regular.woff2"),
        (&a.mono_semibold, "fonts/IBMPlexMono-SemiBold.woff2"),
        (&a.mono_license, "fonts/IBMPlexMono-OFL.txt"),
    ] {
        publish_file(from, out, path, &mut assets)?;
    }

    let theme_src = a
        .repo
        .join("desktop/aizome/crates/aizome-theme/src/lib.rs");
    let theme = theme::Theme::parse(
        &fs::read_to_string(&theme_src)
            .with_context(|| format!("reading {}", theme_src.display()))?,
    )?;
    let symbols: Vec<&Path> = a.symbols.iter().map(PathBuf::as_path).collect();
    let mut site = Site {
        theme,
        kanji: glyphs::Outliner::japanese(&a.cjk, 600.0)?,
        symbols: glyphs::Outliner::fonts(&symbols, 400.0)?,
        revision: a.revision.clone(),
        date: a.date.clone(),
        images: IMAGES.into_iter().collect(),
        repo: a.repo.clone(),
        assets,
    };
    let mut written = Vec::new();

    let mut css = include_str!("../style.css").to_owned();
    // The stylesheet names the fonts by their plain paths.
    for (path, url) in &site.assets {
        css = css.replace(&format!("\"/{path}\""), &format!("\"{url}\""));
    }
    css.push('\n');
    css.push_str(&site.theme.css()?);
    // Hashed too: Cloudflare otherwise lets browsers keep a stylesheet for
    // hours, which could pair new pages with old styles.
    let name = publish(out, "site.css", css.as_bytes(), &mut site.assets)?;
    written.push((name, css));
    // The bar's Night/Day switch, the one script browsers get.
    let script = include_str!("../theme.js");
    let name = publish(out, "theme.js", script.as_bytes(), &mut site.assets)?;
    written.push((name, script.to_owned()));

    let mut pages = vec![
        pages::home(&site)?,
        pages::desktop(&site)?,
        pages::apps_index(&site)?,
    ];
    for (i, _) in apps::APPS.iter().enumerate() {
        pages.push(pages::app(&site, i)?);
    }
    pages.extend([
        pages::keys(&site)?,
        pages::design(&site)?,
        pages::install(&site)?,
        pages::fonts(&site)?,
    ]);
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
    for (name, value) in html::HEADERS {
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

    // Pages link the hashed icon; this keeps the usual address working.
    copy(&a.repo.join(apps::TOOLCHEST_ICON), out, "favicon.svg")?;
    check_links(out, &written)?;
    check_anchors(&written)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashed_names_keep_the_directory_and_extension() {
        let name = hashed_name("images/kura.png", b"rings").unwrap();
        assert!(name.starts_with("images/kura."), "{name}");
        assert!(name.ends_with(".png"), "{name}");
        assert_eq!(name.len(), "images/kura..png".len() + 16);
        assert_eq!(name, hashed_name("images/kura.png", b"rings").unwrap());
    }

    #[test]
    fn dangling_links_are_caught() {
        let out = std::env::temp_dir().join(format!("aizome-site-links-{}", std::process::id()));
        fs::create_dir_all(out.join("apps")).unwrap();
        fs::write(out.join("apps/index.html"), "").unwrap();
        fs::write(out.join("site.css"), "").unwrap();
        let page = |html: &str| vec![("index.html".to_owned(), html.to_owned())];
        let good = check_links(
            &out,
            &page(
                r##"<a href="/apps/#hako">t</a><link href="/site.css"><a href="https://example.com/">e</a><a href="#main">m</a>"##,
            ),
        );
        let bad = check_links(&out, &page(r#"<img src="/images/kura.png">"#));
        fs::remove_dir_all(&out).unwrap();
        good.unwrap();
        assert!(
            bad.unwrap_err()
                .to_string()
                .contains("index.html: /images/kura.png")
        );
    }

    #[test]
    fn dangling_anchors_are_caught() {
        let pages = vec![
            (
                "index.html".to_owned(),
                r##"<a href="#main">m</a><main id="main"></main><a href="/keys/#system">k</a>"##
                    .to_owned(),
            ),
            (
                "keys/index.html".to_owned(),
                r#"<h2 id="system">System</h2>"#.to_owned(),
            ),
        ];
        check_anchors(&pages).unwrap();
        let mut bad = pages.clone();
        bad[0].1.push_str(r##"<a href="/keys/#gone">g</a>"##);
        assert!(
            check_anchors(&bad)
                .unwrap_err()
                .to_string()
                .contains("/keys/#gone")
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
