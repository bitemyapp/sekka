//! The page shell: Aizome's menu bar as the site header, and the footer.

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

pub const ORIGIN: &str = "https://aizome.computer";
pub const REPO: &str = "https://github.com/bitemyapp/sekka";
/// Aizome's own directory in Sekka's repository.
pub const SOURCE: &str = "https://github.com/bitemyapp/sekka/tree/main/desktop/aizome";
pub const SEKKA: &str = "https://sekka.computer/";
pub const SEKKA_DOWNLOAD: &str = "https://sekka.computer/download/";
/// An invite to the Sekka Community Discord server.
pub const DISCORD: &str = "https://discord.gg/baTc5rfqs";

/// The bar's menus: the site's sections, as an app's menus sit in the bar.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Menu {
    Home,
    Desktop,
    Apps,
    Keys,
    Design,
    Performance,
    Install,
}

pub const MENUS: [(Menu, &str, &str); 6] = [
    (Menu::Desktop, "/desktop/", "Desktop"),
    (Menu::Apps, "/apps/", "Apps"),
    (Menu::Keys, "/keys/", "Keys"),
    (Menu::Design, "/design/", "Design"),
    (Menu::Performance, "/performance/", "Performance"),
    (Menu::Install, "/install/", "Install"),
];

pub struct Page {
    /// Site path, such as "/" or "/apps/hako/".
    pub path: String,
    pub title: String,
    pub description: String,
    pub menu: Menu,
    pub body: String,
}

/// Every response's headers, for static files through _headers.
pub const HEADERS: &[(&str, &str)] = &[
    (
        "Content-Security-Policy",
        "default-src 'none'; script-src 'self'; style-src 'self'; img-src 'self'; font-src 'self'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'",
    ),
    ("Referrer-Policy", "no-referrer"),
    ("X-Content-Type-Options", "nosniff"),
    (
        "Permissions-Policy",
        "camera=(), microphone=(), geolocation=(), browsing-topics=()",
    ),
    ("Cross-Origin-Opener-Policy", "same-origin"),
    ("Strict-Transport-Security", "max-age=31536000"),
];

/// The bar's theme switch shows the theme in use: a moon for Night, a sun
/// for Day.
const MOON: &str = r#"<svg class="theme-icon" viewBox="0 0 16 16" width="14" height="14" aria-hidden="true"><path fill="currentColor" d="M14 10.2A6.5 6.5 0 1 1 5.8 2 6 6 0 0 0 14 10.2Z"/></svg>"#;
const SUN: &str = r#"<svg class="theme-icon" viewBox="0 0 16 16" width="14" height="14" aria-hidden="true"><circle cx="8" cy="8" r="3" fill="currentColor"/><path fill="none" stroke="currentColor" stroke-width="1.5" d="M8 .75v2.5M8 12.75v2.5M.75 8h2.5M12.75 8h2.5M2.87 2.87l1.77 1.77M11.36 11.36l1.77 1.77M2.87 13.13l1.77-1.77M11.36 4.64l1.77-1.77"/></svg>"#;

/// Discord's mark, as Simple Icons draws it (CC0), in the button's text
/// colour.
const DISCORD_ICON: &str = r#"<svg class="discord-icon" viewBox="0 0 24 24" width="14" height="14" aria-hidden="true"><path fill="currentColor" d="M20.317 4.3698a19.7913 19.7913 0 00-4.8851-1.5152.0741.0741 0 00-.0785.0371c-.211.3753-.4447.8648-.6083 1.2495-1.8447-.2762-3.68-.2762-5.4868 0-.1636-.3933-.4058-.8742-.6177-1.2495a.077.077 0 00-.0785-.037 19.7363 19.7363 0 00-4.8852 1.515.0699.0699 0 00-.0321.0277C.5334 9.0458-.319 13.5799.0992 18.0578a.0824.0824 0 00.0312.0561c2.0528 1.5076 4.0413 2.4228 5.9929 3.0294a.0777.0777 0 00.0842-.0276c.4616-.6304.8731-1.2952 1.226-1.9942a.076.076 0 00-.0416-.1057c-.6528-.2476-1.2743-.5495-1.8722-.8923a.077.077 0 01-.0076-.1277c.1258-.0943.2517-.1923.3718-.2914a.0743.0743 0 01.0776-.0105c3.9278 1.7933 8.18 1.7933 12.0614 0a.0739.0739 0 01.0785.0095c.1202.099.246.1981.3728.2924a.077.077 0 01-.0066.1276 12.2986 12.2986 0 01-1.873.8914.0766.0766 0 00-.0407.1067c.3604.698.7719 1.3628 1.225 1.9932a.076.076 0 00.0842.0286c1.961-.6067 3.9495-1.5219 6.0023-3.0294a.077.077 0 00.0313-.0552c.5004-5.177-.8382-9.6739-3.5485-13.6604a.061.061 0 00-.0312-.0286zM8.02 15.3312c-1.1825 0-2.1569-1.0857-2.1569-2.419 0-1.3332.9555-2.4189 2.157-2.4189 1.2108 0 2.1757 1.0952 2.1568 2.419 0 1.3332-.9555 2.4189-2.1569 2.4189zm7.9748 0c-1.1825 0-2.1569-1.0857-2.1569-2.419 0-1.3332.9554-2.4189 2.1569-2.4189 1.2108 0 2.1757 1.0952 2.1568 2.419 0 1.3332-.946 2.4189-2.1568 2.4189Z"/></svg>"#;

/// The phone menu's caret, which turns over while the menu is open.
const CARET: &str = r#"<svg class="pages-caret" viewBox="0 0 10 6" width="10" height="6" aria-hidden="true"><path fill="none" stroke="currentColor" stroke-width="1.5" d="M1 1l4 4 4-4"/></svg>"#;

/// Aizome's mark: an amber tab over three indigo stripes, on a 4 × 4
/// module grid (aizome-theme's `Brand::mark_rects`), in the theme's brand
/// colours.
pub fn mark(class: &str, label: Option<&str>) -> String {
    let a11y = match label {
        Some(l) => format!(r#" role="img" aria-label="{}""#, esc(l)),
        None => String::from(r#" aria-hidden="true" focusable="false""#),
    };
    format!(
        r#"<svg class="mark {class}" viewBox="0 0 4 4"{a11y}><rect class="mk-amber" x="0" y="0" width="2" height="1"/><rect class="mk-pale" x="0" y="1" width="4" height="1"/><rect class="mk-mid" x="0" y="2" width="4" height="1"/><rect class="mk-deep" x="0" y="3" width="4" height="1"/></svg>"#
    )
}

pub fn shell(site: &Site, page: &Page) -> Result<String> {
    let mut menus = String::new();
    // On phones the menus drop down from the one this page is in, or from
    // "Menu" on a page that is in none.
    let mut here = "Menu";
    for (menu, path, label) in MENUS {
        let current = if menu == page.menu {
            here = label;
            r#" aria-current="page""#
        } else {
            ""
        };
        menus.push_str(&format!(
            r#"<a class="menu" href="{path}"{current}>{label}</a>"#
        ));
    }
    let home_current = if page.path == "/" {
        r#" aria-current="page""#
    } else {
        ""
    };
    let body = site.render(&page.body)?;
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
<link rel="preload" href="{sans}" as="font" type="font/woff2" crossorigin>
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
<header class="menubar">
  <a class="home" href="/"{home_current}>{mark}<span class="home-name">Aizome</span></a>
  <nav class="menus" aria-label="Pages">{menus}</nav>
  <details class="pages"><summary><span class="visually-hidden">Pages: </span>{here}{CARET}</summary><nav class="pages-menu" aria-label="Pages">{menus}</nav></details>
  <a class="sibling" href="{SEKKA}"><span class="sibling-long">sekka.computer</span><span class="sibling-short">Sekka</span></a>
  <a class="discord" href="{DISCORD}" aria-label="Sekka Community on Discord">{DISCORD_ICON}<span class="discord-name">Discord</span></a>
  <label class="theme"><input type="checkbox" id="site-theme" role="switch" autocomplete="off" aria-label="Day theme"><span class="theme-night" aria-hidden="true">{MOON}<span class="theme-name">Night</span></span><span class="theme-day" aria-hidden="true">{SUN}<span class="theme-name">Day</span></span></label>
  <p class="clock"><span class="clock-label">built from</span> {rev} · <time datetime="{date}">{date}</time></p>
</header>
<main id="main">
{body}
</main>
<footer class="foot">
  <div class="foot-grid">
    <p class="foot-sekka"><strong>Aizome is Sekka's desktop.</strong> The installer, the download and the rest of the system are at <a href="{SEKKA}">sekka.computer</a>. <a href="{SEKKA_DOWNLOAD}">Get Aizome with Sekka</a>.</p>
    <p><strong>No cookies, no analytics.</strong> The one script remembers Night or Day in your browser and sends it nowhere. Every page, script, font and image here is served from this site; nothing is loaded from anywhere else.</p>
    <p>Aizome is MIT or Apache-2.0, at your option. Archivo and IBM Plex Mono are under the <a href="/fonts/">SIL Open Font License</a>; the kanji and key glyphs are outlines drawn from Noto fonts. Screenshots are real captures, never upscaled; each caption says where it came from.</p>
    <p><a href="{SOURCE}">Source</a> · <a href="{REPO}/issues">Issues</a> · <a href="{DISCORD}">Discord</a> · <a href="/fonts/">Fonts</a> · <a href="{SEKKA}">sekka.computer</a></p>
  </div>
</footer>
</body>
</html>
"##,
        title = esc(&page.title),
        description = esc(&page.description),
        path = page.path,
        mark = mark("home-mark", None),
        rev = site.revision_html(),
        sans = site.asset("fonts/Archivo.woff2")?,
        icon = site.asset("icons/org.aizome.Toolchest.svg")?,
        css = site.asset("site.css")?,
        js = site.asset("theme.js")?,
        date = esc(&site.date),
    ))
}
