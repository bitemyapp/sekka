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
pub const NAV: [(&str, &str); 6] = [
    ("/", "sekka"),
    ("/tatami/", "tatami"),
    ("/yukimi/", "yukimi"),
    ("/design/", "design"),
    ("/status/", "status"),
    ("/download/", "download"),
];

pub const ORIGIN: &str = "https://sekka.computer";

/// The bar's theme switch shows the theme in use: a moon for Night, a sun
/// for Day, in the text colour.
const MOON: &str = r#"<svg class="bar-theme-icon" viewBox="0 0 16 16" width="12" height="12"><path fill="currentColor" d="M14 10.2A6.5 6.5 0 1 1 5.8 2 6 6 0 0 0 14 10.2Z"/></svg>"#;
const SUN: &str = r#"<svg class="bar-theme-icon" viewBox="0 0 16 16" width="12" height="12"><circle cx="8" cy="8" r="3" fill="currentColor"/><path fill="none" stroke="currentColor" stroke-width="1.5" d="M8 .75v2.5M8 12.75v2.5M.75 8h2.5M12.75 8h2.5M2.87 2.87l1.77 1.77M11.36 11.36l1.77 1.77M2.87 13.13l1.77-1.77M11.36 4.64l1.77-1.77"/></svg>"#;

pub use crate::shared::{NIGHTLY, REPO};

pub fn shell(site: &Site, page: &Page) -> Result<String> {
    let mut nav = String::new();
    for (i, (path, label)) in NAV.iter().enumerate() {
        let current = if *path == page.path {
            r#" aria-current="page""#
        } else {
            ""
        };
        nav.push_str(&format!(
            r#"<a class="ws" href="{path}"{current}><span class="ws-n" aria-hidden="true">{}</span>{label}</a>"#,
            i + 1
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
  <p class="bar-clock"><span class="bar-clock-label">built from</span> {rev} · <time datetime="{date}">{date}</time></p>
  <label class="bar-theme"><input type="checkbox" id="site-theme" role="switch" autocomplete="off" aria-label="Day theme"><span class="bar-night" aria-hidden="true">{MOON}<span class="bar-theme-name">night</span></span><span class="bar-day" aria-hidden="true">{SUN}<span class="bar-theme-name">day</span></span></label>
  <a class="bar-source" href="{REPO}">source</a>
</header>
<main id="main">
{body}
</main>
<footer class="foot">
  <div class="foot-grid">
    <p><strong>No cookies, no analytics.</strong> The one script remembers Night or Day in your browser and sends it nowhere. Every page, script, font and image here is served from this site; nothing is loaded from anywhere else.</p>
    <p>Sekka builds on NixOS, Determinate Nix and many other free-software projects. Tatami is inspired by Omarchy. Sekka is not an official NixOS, Determinate Systems, Calamares or Omarchy product.</p>
    <p>Genkan is GPL-3.0-or-later; Tatami and Yukimi are MIT OR Apache-2.0. JetBrains Mono and Atkinson Hyperlegible Next are under the <a href="/fonts/">SIL Open Font License</a>.</p>
    <p><a href="{REPO}">Source</a> · <a href="{REPO}/issues">Issues</a> · <a href="/design/">Brand</a></p>
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
