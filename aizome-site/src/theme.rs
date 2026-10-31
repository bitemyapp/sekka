//! Aizome's colours, read from the desktop's own theme crate
//! (desktop/aizome/crates/aizome-theme/src/lib.rs) when the site is built,
//! so the site and the desktop cannot drift apart. The crate's palettes are
//! Rust constants; this reads the `name: Rgba::hex(0x…)` lines of each one
//! and fails the build if a colour the site uses has gone.

use crate::color::{self, Rgba};
use anyhow::{Context, Result, bail};
use std::fmt::Write;

/// One palette, in the crate's order, names as the crate writes them.
pub struct Palette {
    pub id: &'static str,
    pub name: &'static str,
    pub colors: Vec<(String, Rgba, String)>,
}

pub struct Theme {
    /// Night, Day and High Contrast.
    pub palettes: Vec<Palette>,
    /// The mark's colours, by Night and by Day.
    pub brand: Vec<Palette>,
    /// The indigos and the amber, with their dye names.
    pub dyes: Vec<Dye>,
    /// The four focus colours, by Night and by Day.
    pub focus: Vec<(String, String, String)>,
    /// What each palette colour is for, from the crate's doc comments.
    pub docs: Vec<(String, String)>,
    /// Metrics in points, with what they measure.
    pub metrics: Vec<(String, String, String)>,
}

/// `/// comment` lines and the `pub name` that follows them, between
/// `marker` and the closing brace.
fn documented(src: &str, marker: &str) -> Result<Vec<(String, String, String)>> {
    let start = src
        .find(marker)
        .with_context(|| format!("the theme crate has no `{marker}`"))?
        + marker.len();
    let mut out = Vec::new();
    let mut doc = String::new();
    for line in src[start..].lines().skip(1) {
        let line = line.trim();
        if line.starts_with('}') {
            break;
        }
        if let Some(d) = line.strip_prefix("///") {
            if !doc.is_empty() {
                doc.push(' ');
            }
            doc.push_str(d.trim());
            continue;
        }
        let Some(rest) = line.strip_prefix("pub ") else {
            doc.clear();
            continue;
        };
        let rest = rest.strip_prefix("const ").unwrap_or(rest);
        let Some((name, value)) = rest.split_once(':') else {
            doc.clear();
            continue;
        };
        let value = value
            .split_once('=')
            .map(|(_, v)| v.trim().trim_end_matches(';').to_owned())
            .unwrap_or_default();
        out.push((name.trim().to_owned(), value, std::mem::take(&mut doc)));
    }
    Ok(out)
}

pub struct Dye {
    pub name: String,
    pub kanji: String,
    pub hex: String,
}

/// Every palette colour the stylesheet names; the build fails without one.
const USED: [&str; 26] = [
    "bar",
    "surface",
    "chrome",
    "raised",
    "sunk",
    "line",
    "line2",
    "bevel_hi",
    "bevel_lo",
    "slab",
    "text",
    "text2",
    "text3",
    "faint",
    "tab_idle",
    "tab_idle_text",
    "selection",
    "on_selection",
    "menu_title",
    "on_menu_title",
    "focus",
    "on_focus",
    "blue",
    "green",
    "red",
    "desktop",
];

/// The text the site sets, and on what. Each must reach `min`, or the
/// build fails; the design page prints the ratios.
pub const PAIRS: &[(&str, &str, f64, &str)] = &[
    ("text", "surface", 7.0, "Body text"),
    ("text", "desktop", 7.0, "Text on the desktop"),
    ("text", "chrome", 7.0, "The Deskbar"),
    ("text", "sunk", 7.0, "Code"),
    ("text2", "surface", 4.5, "Secondary text"),
    ("text2", "bar", 4.5, "The footer"),
    ("text3", "surface", 4.5, "Captions and notes"),
    ("text3", "chrome", 4.5, "Notes in the Deskbar"),
    ("text3", "raised", 4.5, "Notes on a highlighted row"),
    ("text3", "sunk", 4.5, "Notes in a well"),
    ("text3", "bar", 4.5, "Notes in the menu bar"),
    ("blue", "surface", 4.5, "Links"),
    ("blue", "raised", 4.5, "Links on a highlighted row"),
    ("text", "bar", 7.0, "The menu bar"),
    ("on_focus", "focus", 4.5, "The focused window's tab"),
    ("tab_idle_text", "tab_idle", 4.5, "Other windows' tabs"),
    ("on_selection", "selection", 4.5, "The current page, buttons"),
];

/// `0xRRGGBB` or `0xRRGGBB, alpha` inside `Rgba::hex(…)`/`Rgba::hexa(…)`.
fn rgba(value: &str) -> Option<Rgba> {
    let inner = value
        .strip_prefix("Rgba::hexa(")
        .or_else(|| value.strip_prefix("Rgba::hex("))?;
    let inner = inner.split(')').next()?;
    let mut parts = inner.split(',').map(str::trim);
    let hex = parts.next()?.strip_prefix("0x")?;
    let rgb = u32::from_str_radix(hex, 16).ok()?;
    let a = match parts.next() {
        Some(a) => a.parse::<f64>().ok()?,
        None => 1.0,
    };
    Some(Rgba {
        r: f64::from((rgb >> 16) & 0xff) / 255.0,
        g: f64::from((rgb >> 8) & 0xff) / 255.0,
        b: f64::from(rgb & 0xff) / 255.0,
        a,
    })
}

/// The `name: Rgba::…` entries of the block that opens after `marker`, up
/// to the line that closes it.
fn block(src: &str, marker: &str) -> Result<Vec<(String, Rgba, String)>> {
    let start = src
        .find(marker)
        .with_context(|| format!("the theme crate has no `{marker}`"))?
        + marker.len();
    let mut out = Vec::new();
    for line in src[start..].lines().skip(1) {
        let line = line.trim();
        if line.starts_with('}') {
            break;
        }
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim().trim_end_matches(',');
        if let Some(c) = rgba(value) {
            out.push((name.trim().to_owned(), c, css_color(c)));
        }
    }
    if out.is_empty() {
        bail!("`{marker}` lists no colours");
    }
    Ok(out)
}

pub fn css_color(c: Rgba) -> String {
    let byte = |v: f64| (v * 255.0).round() as u8;
    if c.a >= 1.0 {
        format!("#{:02x}{:02x}{:02x}", byte(c.r), byte(c.g), byte(c.b))
    } else {
        format!(
            "rgb({} {} {} / {})",
            byte(c.r),
            byte(c.g),
            byte(c.b),
            (c.a * 1000.0).round() / 1000.0
        )
    }
}

impl Theme {
    pub fn parse(src: &str) -> Result<Self> {
        let palettes = vec![
            Palette {
                id: "night",
                name: "Night",
                colors: block(src, "pub const NIGHT: Palette = Palette {")?,
            },
            Palette {
                id: "day",
                name: "Day",
                colors: block(src, "pub const DAY: Palette = Palette {")?,
            },
            Palette {
                id: "contrast",
                name: "High Contrast",
                colors: block(src, "pub const HIGH_CONTRAST: Palette = Palette {")?,
            },
        ];
        let brand = vec![
            Palette {
                id: "night",
                name: "Night",
                colors: block(src, "_ => Brand {")?,
            },
            Palette {
                id: "day",
                name: "Day",
                colors: block(src, "ThemeName::Day => Brand {")?,
            },
        ];
        let mut dyes = Vec::new();
        let ramp = src
            .find("pub const DYE_RAMP")
            .context("the theme crate has no DYE_RAMP")?;
        for line in src[ramp..].lines().skip(1) {
            let line = line.trim();
            if line.starts_with(']') {
                break;
            }
            // ("kamenozoki", "甕覗", Rgba::hex(0xa9c4ff)),
            let fields: Vec<&str> = line.split('"').collect();
            if fields.len() < 5 {
                continue;
            }
            let c = rgba(fields[4].trim_start_matches([',', ' ']))
                .with_context(|| format!("cannot read the dye {}", fields[1]))?;
            dyes.push(Dye {
                name: fields[1].to_owned(),
                kanji: fields[3].to_owned(),
                hex: css_color(c),
            });
        }
        let mut focus = Vec::new();
        for (name, label) in [
            ("Amber", "amber"),
            ("Cyan", "cyan"),
            ("Rose", "rose"),
            ("Lime", "lime"),
        ] {
            let mut by = [String::new(), String::new()];
            for (i, day) in ["false", "true"].iter().enumerate() {
                let key = format!("(FocusColor::{name}, {day}) => ");
                let at = src
                    .find(&key)
                    .with_context(|| format!("no focus colour {name}"))?;
                let rest = &src[at + key.len()..];
                let c = rgba(rest.lines().next().unwrap_or_default().trim())
                    .with_context(|| format!("cannot read the focus colour {name}"))?;
                by[i] = css_color(c);
            }
            let [night, day] = by;
            focus.push((label.to_owned(), night, day));
        }
        let docs = documented(src, "pub struct Palette {")?
            .into_iter()
            .map(|(name, _, doc)| (name, doc))
            .collect();
        let metrics = documented(src, "pub mod metrics {")?
            .into_iter()
            .map(|(name, value, doc)| (name, value.trim_end_matches(".0").to_owned(), doc))
            .collect();
        let theme = Theme {
            palettes,
            brand,
            dyes,
            focus,
            docs,
            metrics,
        };
        for p in &theme.palettes {
            for name in USED {
                theme.get(p.id, name)?;
            }
        }
        for &(fg, bg, min, what) in PAIRS {
            for p in &theme.palettes {
                let ratio = theme.contrast(p.id, fg, bg)?;
                if ratio < min {
                    bail!(
                        "{what} ({fg} on {bg}) is {ratio:.2}:1 in {}, short of {min}:1",
                        p.name
                    );
                }
            }
        }
        Ok(theme)
    }

    pub fn palette(&self, id: &str) -> Result<&Palette> {
        self.palettes
            .iter()
            .find(|p| p.id == id)
            .with_context(|| format!("no palette {id}"))
    }

    pub fn get(&self, palette: &str, name: &str) -> Result<Rgba> {
        self.palette(palette)?
            .colors
            .iter()
            .find(|(n, _, _)| n == name)
            .map(|(_, c, _)| *c)
            .with_context(|| format!("the {palette} palette has no {name}"))
    }

    pub fn contrast(&self, palette: &str, fg: &str, bg: &str) -> Result<f64> {
        Ok(color::contrast(
            self.get(palette, fg)?,
            self.get(palette, bg)?,
        ))
    }

    fn block_css(p: &Palette, prefix: &str) -> String {
        let mut s = String::new();
        for (name, _, css) in &p.colors {
            let _ = writeln!(s, "  --{prefix}{}: {css};", name.replace('_', "-"));
        }
        s
    }

    /// Custom properties: Night by default whatever the system's colour
    /// scheme, as Aizome starts; High Contrast when the visitor asks for
    /// more contrast; Day when the bar's switch is on (checked on the page,
    /// or remembered by theme.js as `data-theme`). The design page's
    /// palette swatches are classes, since the security policy allows no
    /// inline styles.
    pub fn css(&self) -> Result<String> {
        let night = self.palette("night")?;
        let day = self.palette("day")?;
        let contrast = self.palette("contrast")?;
        let brand = |id: &str| self.brand.iter().find(|b| b.id == id);
        let mut s = String::from(
            "/* Generated from desktop/aizome/crates/aizome-theme/src/lib.rs. */\n:root {\n  color-scheme: dark;\n",
        );
        s += &Self::block_css(night, "");
        if let Some(b) = brand("night") {
            s += &Self::block_css(b, "brand-");
        }
        for d in &self.dyes {
            let _ = writeln!(s, "  --dye-{}: {};", d.name, d.hex);
        }
        s += "}\n";
        let _ = writeln!(
            s,
            "@media (prefers-contrast: more) {{\n:root {{\n{}}}\n}}",
            Self::block_css(contrast, "")
        );
        let mut day_block = String::from("  color-scheme: light;\n");
        day_block += &Self::block_css(day, "");
        if let Some(b) = brand("day") {
            day_block += &Self::block_css(b, "brand-");
        }
        let _ = writeln!(
            s,
            ":root[data-theme=\"day\"],\n:root:has(#site-theme:checked) {{\n{day_block}}}"
        );
        for p in &self.palettes {
            for (name, _, css) in &p.colors {
                let _ = writeln!(
                    s,
                    ".sw-{}-{} {{ background: {css}; }}",
                    p.id,
                    name.replace('_', "-")
                );
            }
        }
        for d in &self.dyes {
            let _ = writeln!(s, ".sw-dye-{} {{ background: {}; }}", d.name, d.hex);
        }
        for (name, night, day) in &self.focus {
            let _ = writeln!(
                s,
                ".sw-focus-{name}-night {{ background: {night}; }}\n.sw-focus-{name}-day {{ background: {day}; }}"
            );
        }
        Ok(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
pub const NIGHT: Palette = Palette {
    name: ThemeName::Night,
    bar: Rgba::hex(0x1c1e2c),
    slab: Rgba::hexa(0x000000, 0.5),
};
"#;

    #[test]
    fn reads_a_palette_block() {
        let colors = block(SAMPLE, "pub const NIGHT: Palette = Palette {").unwrap();
        assert_eq!(colors.len(), 2);
        assert_eq!(colors[0].0, "bar");
        assert_eq!(colors[0].2, "#1c1e2c");
        assert_eq!(colors[1].2, "rgb(0 0 0 / 0.5)");
    }

    #[test]
    fn a_missing_block_fails() {
        assert!(block(SAMPLE, "pub const DAY: Palette = Palette {").is_err());
    }
}
