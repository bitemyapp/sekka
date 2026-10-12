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

/// The workspaces in the bar are the site's pages, as Tatami's are 1 to 5.
pub const NAV: [(&str, &str); 4] = [
    ("/", "sekka"),
    ("/tatami/", "tatami"),
    ("/design/", "design"),
    ("/status/", "status"),
];

pub const ORIGIN: &str = "https://sekka.computer";
pub const REPO: &str = "https://github.com/bitemyapp/sekka";

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
<link rel="icon" href="/favicon.svg" type="image/svg+xml">
<link rel="preload" href="/fonts/JetBrainsMono.ttf" as="font" type="font/ttf" crossorigin>
<link rel="stylesheet" href="/site.css">
<meta property="og:title" content="{title}">
<meta property="og:description" content="{description}">
<meta property="og:url" content="{ORIGIN}{path}">
<meta property="og:type" content="website">
<meta name="color-scheme" content="dark light">
</head>
<body>
<a class="skip" href="#main">Skip to the content</a>
<header class="bar">
  <a class="bar-menu" href="/" aria-label="Sekka, home">{crest}</a>
  <nav class="bar-ws" aria-label="Pages">{nav}</nav>
  <p class="bar-clock"><span class="bar-clock-label">built from</span> {rev} · <time datetime="{date}">{date}</time></p>
  <a class="bar-source" href="{REPO}">source</a>
</header>
<main id="main">
{body}
</main>
<footer class="foot">
  <div class="foot-grid">
    <p><strong>No cookies, no analytics, no scripts.</strong> Every page, font and image here is served from this site; nothing is loaded from anywhere else.</p>
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
        date = esc(&site.date),
        body = page.body,
    ))
}
