//! The brand tokens (docs/brand/tokens.json), read directly so the site and
//! the brand cannot drift apart, and turned into CSS custom properties.

use crate::color::{self, Rgba};
use anyhow::{Context, Result, anyhow, bail};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fmt::Write;
use std::path::Path;

pub struct Theme {
    pub id: String,
    pub name: String,
}

pub struct ColorToken {
    pub name: String,
    pub values: BTreeMap<String, String>,
    pub usage: String,
}

pub struct Plain {
    pub name: String,
    pub value: String,
    pub usage: String,
}

pub struct Style {
    pub name: String,
    pub family: String,
    pub size: String,
    pub line_height: Option<String>,
    pub weight: Option<String>,
    pub letter_spacing: Option<String>,
    pub sample: Option<String>,
    pub usage: String,
}

pub struct TypeGroup {
    pub name: String,
    pub styles: Vec<Style>,
}

pub struct Tokens {
    pub themes: Vec<Theme>,
    pub colors: Vec<ColorToken>,
    pub families: Vec<(String, String)>,
    pub groups: Vec<TypeGroup>,
    /// spacing, radius, size, border: each a list of plain values.
    pub plain: Vec<(String, Vec<Plain>)>,
}

/// The grounds every text colour is checked against.
pub const GROUNDS: [&str; 3] = ["surface", "surface-raised", "selection"];

fn text(v: &Value, key: &str) -> Option<String> {
    match v.get(key)? {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

fn plain_list(v: &Value) -> Result<Vec<Plain>> {
    v.get("tokens")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("token family without a tokens list"))?
        .iter()
        .map(|t| {
            Ok(Plain {
                name: text(t, "name").context("token without a name")?,
                value: text(t, "value").context("token without a value")?,
                usage: text(t, "usage").unwrap_or_default(),
            })
        })
        .collect()
}

impl Tokens {
    pub fn load(path: &Path) -> Result<Self> {
        let doc: Value = serde_json::from_slice(
            &std::fs::read(path).with_context(|| format!("reading {}", path.display()))?,
        )?;
        let color = &doc["color"];
        let themes = color["themes"]
            .as_array()
            .context("no colour themes")?
            .iter()
            .map(|t| Theme {
                id: text(t, "id").unwrap_or_default(),
                name: text(t, "name").unwrap_or_default(),
            })
            .collect::<Vec<_>>();
        let colors = color["tokens"]
            .as_array()
            .context("no colour tokens")?
            .iter()
            .map(|t| {
                let values = match &t["value"] {
                    Value::String(s) => BTreeMap::from([(themes[0].id.clone(), s.clone())]),
                    Value::Object(map) => map
                        .iter()
                        .filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_owned())))
                        .collect(),
                    _ => BTreeMap::new(),
                };
                Ok(ColorToken {
                    name: text(t, "name").context("colour without a name")?,
                    values,
                    usage: text(t, "usage").unwrap_or_default(),
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let ty = &doc["type"];
        let families = ty["families"]
            .as_object()
            .context("no font families")?
            .iter()
            .filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_owned())))
            .collect();
        let groups = ty["groups"]
            .as_array()
            .context("no type groups")?
            .iter()
            .map(|g| {
                let family = text(g, "family").unwrap_or_default();
                TypeGroup {
                    name: text(g, "name").unwrap_or_default(),
                    styles: g["styles"]
                        .as_array()
                        .map(|styles| {
                            styles
                                .iter()
                                .map(|s| Style {
                                    name: text(s, "name").unwrap_or_default(),
                                    family: text(s, "family").unwrap_or_else(|| family.clone()),
                                    size: text(s, "fontSize").unwrap_or_default(),
                                    line_height: text(s, "lineHeight"),
                                    weight: text(s, "fontWeight"),
                                    letter_spacing: text(s, "letterSpacing"),
                                    sample: text(s, "sample"),
                                    usage: text(s, "usage").unwrap_or_default(),
                                })
                                .collect()
                        })
                        .unwrap_or_default(),
                }
            })
            .collect();
        let mut plain = Vec::new();
        for family in ["spacing", "radius", "size", "border"] {
            if let Some(v) = doc.get(family) {
                plain.push((family.to_owned(), plain_list(v)?));
            }
        }
        let tokens = Tokens {
            themes,
            colors,
            families,
            groups,
            plain,
        };
        // Every colour must resolve in every theme before anything is written.
        for t in &tokens.themes {
            for c in &tokens.colors {
                tokens.resolve(&c.name, &t.id)?;
            }
        }
        Ok(tokens)
    }

    pub fn color(&self, name: &str) -> Result<&ColorToken> {
        self.colors
            .iter()
            .find(|c| c.name == name)
            .ok_or_else(|| anyhow!("no colour token {name}"))
    }

    /// The written value for a theme; a missing theme inherits the first.
    pub fn raw(&self, name: &str, theme: &str) -> Result<&str> {
        let c = self.color(name)?;
        c.values
            .get(theme)
            .or_else(|| c.values.get(&self.themes[0].id))
            .map(String::as_str)
            .ok_or_else(|| anyhow!("{name} has no value"))
    }

    pub fn resolve(&self, name: &str, theme: &str) -> Result<Rgba> {
        let mut current = name.to_owned();
        for _ in 0..16 {
            let raw = self.raw(&current, theme)?;
            if let Some(alias) = raw.strip_prefix('{').and_then(|r| r.strip_suffix('}')) {
                current = alias.to_owned();
                continue;
            }
            return color::parse(raw).ok_or_else(|| anyhow!("{name}: cannot read {raw}"));
        }
        bail!("{name}: alias chain too deep")
    }

    /// The colour's hex as resolved for a theme, aliases followed.
    pub fn hex(&self, name: &str, theme: &str) -> Result<String> {
        let mut current = name.to_owned();
        for _ in 0..16 {
            let raw = self.raw(&current, theme)?;
            match raw.strip_prefix('{').and_then(|r| r.strip_suffix('}')) {
                Some(alias) => current = alias.to_owned(),
                None => return Ok(raw.to_owned()),
            }
        }
        bail!("{name}: alias chain too deep")
    }

    pub fn contrast(&self, fg: &str, bg: &str, theme: &str) -> Result<f64> {
        Ok(color::contrast(
            self.resolve(fg, theme)?,
            self.resolve(bg, theme)?,
        ))
    }

    fn css_value(raw: &str) -> String {
        match raw.strip_prefix('{').and_then(|r| r.strip_suffix('}')) {
            Some(alias) => format!("var(--{alias})"),
            None => raw.to_owned(),
        }
    }

    fn theme_block(&self, theme: &str) -> Result<String> {
        let ground = self.resolve("surface", theme)?;
        let scheme = if color::luminance(ground) > 0.5 {
            "light"
        } else {
            "dark"
        };
        let mut s = format!("  color-scheme: {scheme};\n");
        for c in &self.colors {
            writeln!(
                s,
                "  --{}: {};",
                c.name,
                Self::css_value(self.raw(&c.name, theme)?)
            )?;
        }
        Ok(s)
    }

    /// Custom properties for every token. Night is the default, whatever
    /// the system's colour scheme; High contrast follows a contrast
    /// preference. Day is the bar's switch: checked on the page, or the
    /// choice theme.js remembered, as `data-theme`. The design page's theme
    /// picker, last, overrides them all on its page.
    pub fn css(&self) -> Result<String> {
        let first = &self.themes[0].id;
        let mut s = String::from("/* Generated from docs/brand/tokens.json. */\n:root {\n");
        s += &self.theme_block(first)?;
        for (_, list) in &self.plain {
            for p in list {
                writeln!(s, "  --{}: {};", p.name, p.value)?;
            }
        }
        for (key, stack) in &self.families {
            writeln!(s, "  --font-{key}: {stack};")?;
        }
        s += "}\n";
        if self.themes.iter().any(|t| t.id == "contrast") {
            writeln!(
                s,
                "@media (prefers-contrast: more) {{\n:root {{\n{}}}\n}}",
                self.theme_block("contrast")?
            )?;
        }
        if self.themes.iter().any(|t| t.id == "day") {
            writeln!(
                s,
                ":root[data-theme=\"day\"],\n:root:has(#site-theme:checked) {{\n{}}}",
                self.theme_block("day")?
            )?;
        }
        for t in &self.themes {
            writeln!(
                s,
                ":root:has(#theme-{}:checked) {{\n{}}}",
                t.id,
                self.theme_block(&t.id)?
            )?;
        }
        for g in &self.groups {
            for st in &g.styles {
                write!(
                    s,
                    ".type-{} {{ font-family: var(--font-{}); font-size: {};",
                    st.name, st.family, st.size
                )?;
                if let Some(v) = &st.line_height {
                    write!(s, " line-height: {v};")?;
                }
                if let Some(v) = &st.weight {
                    write!(s, " font-weight: {v};")?;
                }
                if let Some(v) = &st.letter_spacing {
                    write!(s, " letter-spacing: {v};")?;
                }
                s += " }\n";
            }
        }
        Ok(s)
    }
}
