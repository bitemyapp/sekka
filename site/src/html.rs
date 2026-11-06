//! The page shell: Tatami's bar as the site header, and the footer.

use crate::Site;
use anyhow::Result;

pub fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

pub struct Page {
    /// Site path, such as "/" or "/tatami/".
    pub path: &'static str,
    pub title: String,
    pub description: &'static str,
    pub body: String,
}

/// The workspaces in the bar are the site's pages, numbered as Tatami's are.
pub const NAV: [(&str, &str); 7] = [
    ("/", "sekka"),
    ("/aizome/", "aizome"),
    ("/tatami/", "tatami"),
    ("/yukimi/", "yukimi"),
    ("/design/", "design"),
    ("/status/", "status"),
    ("/download/", "download"),
];

pub const ORIGIN: &str = "https://sekka.computer";

/// Aizome's own site, which links back here.
pub const AIZOME: &str = "https://aizome.computer";

/// An invite to the Sekka Community Discord server.
pub const DISCORD: &str = "https://discord.gg/baTc5rfqs";

/// The bar's theme switch shows the theme in use: a moon for Night, a sun
/// for Day, in the text colour.
const MOON: &str = r#"<svg class="bar-theme-icon" viewBox="0 0 16 16" width="12" height="12"><path fill="currentColor" d="M14 10.2A6.5 6.5 0 1 1 5.8 2 6 6 0 0 0 14 10.2Z"/></svg>"#;
const SUN: &str = r#"<svg class="bar-theme-icon" viewBox="0 0 16 16" width="12" height="12"><circle cx="8" cy="8" r="3" fill="currentColor"/><path fill="none" stroke="currentColor" stroke-width="1.5" d="M8 .75v2.5M8 12.75v2.5M.75 8h2.5M12.75 8h2.5M2.87 2.87l1.77 1.77M11.36 11.36l1.77 1.77M2.87 13.13l1.77-1.77M11.36 4.64l1.77-1.77"/></svg>"#;

/// Discord's mark, as Simple Icons draws it (CC0), in the chip's text colour.
const DISCORD_ICON: &str = r#"<svg class="bar-discord-icon" viewBox="0 0 24 24" width="13" height="13" aria-hidden="true"><path fill="currentColor" d="M20.317 4.3698a19.7913 19.7913 0 00-4.8851-1.5152.0741.0741 0 00-.0785.0371c-.211.3753-.4447.8648-.6083 1.2495-1.8447-.2762-3.68-.2762-5.4868 0-.1636-.3933-.4058-.8742-.6177-1.2495a.077.077 0 00-.0785-.037 19.7363 19.7363 0 00-4.8852 1.515.0699.0699 0 00-.0321.0277C.5334 9.0458-.319 13.5799.0992 18.0578a.0824.0824 0 00.0312.0561c2.0528 1.5076 4.0413 2.4228 5.9929 3.0294a.0777.0777 0 00.0842-.0276c.4616-.6304.8731-1.2952 1.226-1.9942a.076.076 0 00-.0416-.1057c-.6528-.2476-1.2743-.5495-1.8722-.8923a.077.077 0 01-.0076-.1277c.1258-.0943.2517-.1923.3718-.2914a.0743.0743 0 01.0776-.0105c3.9278 1.7933 8.18 1.7933 12.0614 0a.0739.0739 0 01.0785.0095c.1202.099.246.1981.3728.2924a.077.077 0 01-.0066.1276 12.2986 12.2986 0 01-1.873.8914.0766.0766 0 00-.0407.1067c.3604.698.7719 1.3628 1.225 1.9932a.076.076 0 00.0842.0286c1.961-.6067 3.9495-1.5219 6.0023-3.0294a.077.077 0 00.0313-.0552c.5004-5.177-.8382-9.6739-3.5485-13.6604a.061.061 0 00-.0312-.0286zM8.02 15.3312c-1.1825 0-2.1569-1.0857-2.1569-2.419 0-1.3332.9555-2.4189 2.157-2.4189 1.2108 0 2.1757 1.0952 2.1568 2.419 0 1.3332-.9555 2.4189-2.1569 2.4189zm7.9748 0c-1.1825 0-2.1569-1.0857-2.1569-2.419 0-1.3332.9554-2.4189 2.1569-2.4189 1.2108 0 2.1757 1.0952 2.1568 2.419 0 1.3332-.946 2.4189-2.1568 2.4189Z"/></svg>"#;

/// The phone menu's caret, which turns over while the menu is open.
const CARET: &str = r#"<svg class="bar-pages-caret" viewBox="0 0 10 6" width="10" height="6" aria-hidden="true"><path fill="none" stroke="currentColor" stroke-width="1.5" d="M1 1l4 4 4-4"/></svg>"#;

pub use crate::shared::{NIGHTLY, REPO};

pub fn shell(site: &Site, page: &Page) -> Result<String> {
    let mut nav = String::new();
    // On phones the workspaces drop down from the one this page is, or from
    // "pages" on a page that isn't one.
    let mut here = String::from("pages");
    for (i, (path, label)) in NAV.iter().enumerate() {
        let n = i + 1;
        let current = if *path == page.path {
            here = format!(r#"<span class="ws-n" aria-hidden="true">{n}</span>{label}"#);
            r#" aria-current="page""#
        } else {
            ""
        };
        nav.push_str(&format!(
            r#"<a class="ws" href="{path}"{current}><span class="ws-n" aria-hidden="true">{n}</span>{label}</a>"#
        ));
    }
    let crest = site
        .brand
        .inline("sekka-symbolic.svg", "bar-crest", "Sekka")?;
    Ok(format!(
        r##"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title}</title>
<meta name="description" content="{description}">
<link rel="canonical" href="{ORIGIN}{path}">
<link rel="icon" href="{icon}" type="image/svg+xml">
<link rel="preload" href="{mono}" as="font" type="font/ttf" crossorigin>
<script src="{js}"></script>
<link rel="stylesheet" href="{css}">
<meta property="og:title" content="{title}">
<meta property="og:description" content="{description}">
<meta property="og:url" content="{ORIGIN}{path}">
<meta property="og:type" content="website">
<meta name="color-scheme" content="dark">
</head>
<body>
<a class="skip" href="#main">Skip to the content</a>
<header class="bar">
  <a class="bar-menu" href="/" aria-label="Sekka, home">{crest}</a>
  <nav class="bar-ws" aria-label="Pages">{nav}</nav>
  <details class="bar-pages"><summary><span class="visually-hidden">Pages: </span>{here}{CARET}</summary><nav class="bar-pages-menu" aria-label="Pages">{nav}</nav></details>
  <p class="bar-clock"><span class="bar-clock-label">built from</span> {rev} · <time datetime="{date}">{date}</time></p>
  <a class="bar-discord" href="{DISCORD}" aria-label="Sekka Community on Discord">{DISCORD_ICON}<span class="bar-discord-name">discord</span></a>
  <label class="bar-theme"><input type="checkbox" id="site-theme" role="switch" autocomplete="off" aria-label="Day theme"><span class="bar-night" aria-hidden="true">{MOON}<span class="bar-theme-name">night</span></span><span class="bar-day" aria-hidden="true">{SUN}<span class="bar-theme-name">day</span></span></label>
  <a class="bar-source" href="{REPO}">source</a>
</header>
<main id="main">
{body}
</main>
<footer class="foot">
  <div class="foot-grid">
    <p><strong>No cookies, no analytics.</strong> One script remembers Night or Day in your browser and sends it nowhere; the other plays the video on the home page. Every page, script, font and image here is served from this site; nothing is loaded from anywhere else.</p>
    <p>Sekka builds on NixOS, Determinate Nix and many other free-software projects. Aizome takes its ideas from Haiku, NeXTSTEP and IRIX; Tatami is inspired by Omarchy. Sekka is not an official NixOS, Determinate Systems, Calamares or Omarchy product.</p>
    <p>Genkan is GPL-3.0-or-later; Aizome, Tatami and Yukimi are MIT OR Apache-2.0. JetBrains Mono, Atkinson Hyperlegible Next and the video's fonts are under the <a href="/fonts/">SIL Open Font License</a>.</p>
    <p><a href="{AIZOME}/">aizome.computer</a> · <a href="{REPO}">Source</a> · <a href="{REPO}/issues">Issues</a> · <a href="{DISCORD}">Discord</a> · <a href="/design/">Brand</a></p>
  </div>
</footer>
</body>
</html>
"##,
        title = esc(&page.title),
        description = esc(page.description),
        path = page.path,
        rev = site.revision_html(),
        mono = site.asset("fonts/JetBrainsMono.ttf")?,
        icon = site.asset("brand/sekka-app-icon.svg")?,
        css = site.asset("site.css")?,
        js = site.asset("theme.js")?,
        date = esc(&site.date),
        body = page.body,
    ))
}
