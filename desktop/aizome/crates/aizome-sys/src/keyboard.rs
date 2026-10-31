//! Keyboard layouts, variants, models and XKB options with human names,
//! from xkeyboard-config's registry (`rules/evdev.xml`, the format of
//! `xkb.dtd`), for the Settings app's keyboard panel.
//!
//! - **Where**: `$XKB_CONFIG_ROOT/rules/`, `/run/current-system/sw/share/X11/xkb/rules/`,
//!   `/etc/X11/xkb/rules/`, `/usr/share/X11/xkb/rules/`, each
//!   `$XDG_DATA_DIRS/X11/xkb/rules/`, and as a last resort (it lists the
//!   whole store) the newest `/nix/store/*-xkeyboard-config-<version>`.
//! - **What**: models (name, description, vendor); layouts (name, short
//!   description such as `en`, description, ISO 639 language and ISO 3166
//!   country codes, variants with the same); option groups (name such as
//!   `ctrl`, description, whether several options may be chosen) with
//!   their options (name such as `ctrl:nocaps`, description). A missing
//!   description falls back to the name; items without a name are skipped.
//!   A variant without languages of its own has its layout's.
//! - **Extras**: `evdev.extras.xml` holds rarely used ("exotic") layouts,
//!   variants and options. Merged on request, they are marked `extra`;
//!   extra variants of a known layout join that layout, extra options of a
//!   known group join that group.
//! - **Order**: layouts and variants are sorted by description (ignoring
//!   case); models, option groups and options keep the file's order.
//!
//! Descriptions are in English; translating them is gettext's job (domain
//! `xkeyboard-config`).

use crate::{Error, Result};
use std::path::{Path, PathBuf};

/// A keyboard model (`pc105`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyboardModel {
    pub name: String,
    pub description: String,
    pub vendor: Option<String>,
    pub extra: bool,
}

/// A layout (`us`, `de`) and its variants.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Layout {
    /// The XKB name (`us`).
    pub name: String,
    /// The indicator text (`en`).
    pub short_description: Option<String>,
    /// The human name (`English (US)`).
    pub description: String,
    /// ISO 639-2/3 language codes (`eng`).
    pub languages: Vec<String>,
    /// ISO 3166 country codes (`US`).
    pub countries: Vec<String>,
    /// The variants, sorted by description.
    pub variants: Vec<Variant>,
    /// From `evdev.extras.xml`.
    pub extra: bool,
}

/// A variant of a layout (`intl`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Variant {
    /// The XKB name (`intl`).
    pub name: String,
    pub short_description: Option<String>,
    /// The human name, which includes the layout's
    /// (`English (US, intl., with dead keys)`).
    pub description: String,
    /// Its own languages, or the layout's when it lists none.
    pub languages: Vec<String>,
    pub countries: Vec<String>,
    /// From `evdev.extras.xml`.
    pub extra: bool,
}

/// A group of XKB options (`ctrl`, "Ctrl position").
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OptionGroup {
    pub name: String,
    pub description: String,
    /// Whether several of its options may be chosen at once (otherwise
    /// they exclude each other).
    pub allow_multiple_selection: bool,
    pub options: Vec<XkbOption>,
    pub extra: bool,
}

/// An XKB option (`ctrl:nocaps`, "Caps Lock as Ctrl").
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct XkbOption {
    pub name: String,
    pub description: String,
    pub extra: bool,
}

/// A layout or one of its variants, for a flat "add an input source" list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LayoutChoice<'a> {
    pub layout: &'a str,
    /// `None` for the layout's default variant.
    pub variant: Option<&'a str>,
    pub description: &'a str,
    pub extra: bool,
}

/// Everything in an XKB registry; see the module documentation.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct KeyboardData {
    pub models: Vec<KeyboardModel>,
    /// Sorted by description.
    pub layouts: Vec<Layout>,
    pub option_groups: Vec<OptionGroup>,
    /// The `rules` directory it was read from, if read from files.
    pub source: Option<PathBuf>,
}

impl KeyboardData {
    /// Read `evdev.xml` from the first rules directory found (see the
    /// module documentation).
    pub fn load() -> Result<KeyboardData> {
        KeyboardData::load_with(false)
    }

    /// Like [`load`](Self::load), merging `evdev.extras.xml` if
    /// `include_extras`.
    pub fn load_with(include_extras: bool) -> Result<KeyboardData> {
        let dir = find_rules_dir().ok_or_else(|| Error::NotFound("xkeyboard-config's rules/evdev.xml".into()))?;
        KeyboardData::load_from(&dir, include_extras)
    }

    /// Read `evdev.xml` (and `evdev.extras.xml`, if asked and present) from
    /// a rules directory.
    pub fn load_from(rules_dir: &Path, include_extras: bool) -> Result<KeyboardData> {
        let read = |name: &str| -> Result<String> {
            let path = rules_dir.join(name);
            let bytes = std::fs::read(&path).map_err(|error| file_error(error, &path))?;
            Ok(String::from_utf8_lossy(&bytes).into_owned())
        };
        let mut data = KeyboardData::from_xml(&read("evdev.xml")?)?;
        if include_extras && rules_dir.join("evdev.extras.xml").is_file() {
            data.merge_extras_xml(&read("evdev.extras.xml")?)?;
        }
        data.source = Some(rules_dir.to_owned());
        Ok(data)
    }

    /// Parse registry XML (the contents of `evdev.xml`).
    pub fn from_xml(xml: &str) -> Result<KeyboardData> {
        let mut data = parse_registry(xml, false)?;
        data.sort();
        Ok(data)
    }

    /// Merge registry XML holding extras (`evdev.extras.xml`), marking
    /// what it adds as `extra`.
    pub fn merge_extras_xml(&mut self, xml: &str) -> Result<()> {
        let extras = parse_registry(xml, true)?;
        for model in extras.models {
            if !self.models.iter().any(|known| known.name == model.name) {
                self.models.push(model);
            }
        }
        for layout in extras.layouts {
            match self.layouts.iter_mut().find(|known| known.name == layout.name) {
                Some(known) => {
                    for variant in layout.variants {
                        if !known.variants.iter().any(|v| v.name == variant.name) {
                            known.variants.push(variant);
                        }
                    }
                }
                None => self.layouts.push(layout),
            }
        }
        for group in extras.option_groups {
            match self.option_groups.iter_mut().find(|known| known.name == group.name) {
                Some(known) => {
                    for option in group.options {
                        if !known.options.iter().any(|o| o.name == option.name) {
                            known.options.push(option);
                        }
                    }
                }
                None => self.option_groups.push(group),
            }
        }
        self.sort();
        Ok(())
    }

    fn sort(&mut self) {
        let key = |description: &str| description.to_lowercase();
        for layout in &mut self.layouts {
            layout.variants.sort_by_cached_key(|v| (key(&v.description), v.name.clone()));
        }
        self.layouts.sort_by_cached_key(|l| (key(&l.description), l.name.clone()));
    }

    /// The layout called `name` (`us`).
    pub fn layout(&self, name: &str) -> Option<&Layout> {
        self.layouts.iter().find(|layout| layout.name == name)
    }

    /// A variant of a layout (`us`, `intl`).
    pub fn variant(&self, layout: &str, variant: &str) -> Option<&Variant> {
        self.layout(layout)?.variants.iter().find(|v| v.name == variant)
    }

    /// The human name of a layout and variant (`""` for the default
    /// variant): `describe("us", "intl")` is "English (US, intl., with dead
    /// keys)". Unknown names are shown as they are.
    pub fn describe(&self, layout: &str, variant: &str) -> String {
        let variant = variant.trim();
        let Some(known) = self.layout(layout) else {
            return if variant.is_empty() { layout.to_owned() } else { format!("{layout} ({variant})") };
        };
        if variant.is_empty() {
            return known.description.clone();
        }
        match known.variants.iter().find(|v| v.name == variant) {
            Some(v) => v.description.clone(),
            None => format!("{} ({variant})", known.description),
        }
    }

    /// Every layout and variant as one list, sorted by description.
    pub fn choices(&self) -> Vec<LayoutChoice<'_>> {
        let mut choices: Vec<LayoutChoice<'_>> = Vec::new();
        for layout in &self.layouts {
            choices.push(LayoutChoice {
                layout: &layout.name,
                variant: None,
                description: &layout.description,
                extra: layout.extra,
            });
            for variant in &layout.variants {
                choices.push(LayoutChoice {
                    layout: &layout.name,
                    variant: Some(&variant.name),
                    description: &variant.description,
                    extra: variant.extra,
                });
            }
        }
        choices.sort_by_cached_key(|c| (c.description.to_lowercase(), c.layout, c.variant));
        choices
    }

    /// The option group called `name` (`ctrl`).
    pub fn option_group(&self, name: &str) -> Option<&OptionGroup> {
        self.option_groups.iter().find(|group| group.name == name)
    }

    /// The option called `name` (`ctrl:nocaps`), in any group.
    pub fn option(&self, name: &str) -> Option<&XkbOption> {
        self.option_groups.iter().flat_map(|group| &group.options).find(|option| option.name == name)
    }
}

/// An I/O error naming the file.
fn file_error(error: std::io::Error, path: &Path) -> Error {
    match Error::from(error) {
        Error::NotFound(what) => Error::NotFound(format!("{}: {what}", path.display())),
        Error::Io(what) => Error::Io(format!("{}: {what}", path.display())),
        other => other,
    }
}

fn parse_registry(xml: &str, extra: bool) -> Result<KeyboardData> {
    let options = roxmltree::ParsingOptions { allow_dtd: true, ..roxmltree::ParsingOptions::default() };
    let document = roxmltree::Document::parse_with_options(xml, options)
        .map_err(|error| Error::Failed(format!("not an XKB registry: {error}")))?;
    let root = document.root_element();
    if root.tag_name().name() != "xkbConfigRegistry" {
        return Err(Error::Failed(format!("not an XKB registry: <{}>", root.tag_name().name())));
    }
    let mut data = KeyboardData::default();
    for model in children(root, "modelList").flat_map(|list| children(list, "model")) {
        if let Some(item) = Item::of(model) {
            data.models.push(KeyboardModel {
                vendor: item.text("vendor"),
                description: item.description,
                name: item.name,
                extra,
            });
        }
    }
    for layout in children(root, "layoutList").flat_map(|list| children(list, "layout")) {
        let Some(item) = Item::of(layout) else {
            continue;
        };
        let languages = item.list("languageList", "iso639Id");
        let variants = children(layout, "variantList")
            .flat_map(|list| children(list, "variant"))
            .filter_map(Item::of)
            .map(|variant| {
                let own = variant.list("languageList", "iso639Id");
                Variant {
                    short_description: variant.text("shortDescription"),
                    languages: if own.is_empty() { languages.clone() } else { own },
                    countries: variant.list("countryList", "iso3166Id"),
                    description: variant.description,
                    name: variant.name,
                    extra,
                }
            })
            .collect();
        data.layouts.push(Layout {
            short_description: item.text("shortDescription"),
            countries: item.list("countryList", "iso3166Id"),
            languages,
            variants,
            description: item.description,
            name: item.name,
            extra,
        });
    }
    for group in children(root, "optionList").flat_map(|list| children(list, "group")) {
        let Some(item) = Item::of(group) else {
            continue;
        };
        let options = children(group, "option")
            .filter_map(Item::of)
            .map(|option| XkbOption { name: option.name, description: option.description, extra })
            .collect();
        data.option_groups.push(OptionGroup {
            name: item.name,
            description: item.description,
            allow_multiple_selection: group.attribute("allowMultipleSelection").is_some_and(|v| v.trim() == "true"),
            options,
            extra,
        });
    }
    Ok(data)
}

fn children<'a, 'input>(
    node: roxmltree::Node<'a, 'input>,
    name: &'static str,
) -> impl Iterator<Item = roxmltree::Node<'a, 'input>> {
    node.children().filter(move |child| child.is_element() && child.tag_name().name() == name)
}

/// A `<configItem>`: name, description, and the node to read more from.
struct Item<'a, 'input> {
    config: roxmltree::Node<'a, 'input>,
    name: String,
    description: String,
}

impl<'a, 'input> Item<'a, 'input> {
    fn of(node: roxmltree::Node<'a, 'input>) -> Option<Item<'a, 'input>> {
        let config = children(node, "configItem").next()?;
        let name = element_text(config, "name")?;
        let description = element_text(config, "description").unwrap_or_else(|| name.clone());
        Some(Item { config, name, description })
    }

    fn text(&self, name: &'static str) -> Option<String> {
        element_text(self.config, name)
    }

    fn list(&self, list: &'static str, item: &'static str) -> Vec<String> {
        children(self.config, list)
            .flat_map(|list| children(list, item))
            .filter_map(|node| node.text().map(str::trim).filter(|t| !t.is_empty()).map(str::to_owned))
            .collect()
    }
}

/// The trimmed text of the first `name` child, if not empty.
fn element_text(node: roxmltree::Node, name: &'static str) -> Option<String> {
    let text: String =
        children(node, name).next()?.descendants().filter(|n| n.is_text()).filter_map(|n| n.text()).collect();
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    (!text.is_empty()).then_some(text)
}

/// The first rules directory with an `evdev.xml`; see the module
/// documentation for the order.
pub fn find_rules_dir() -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(root) = std::env::var_os("XKB_CONFIG_ROOT").filter(|root| !root.is_empty()) {
        candidates.push(PathBuf::from(root).join("rules"));
    }
    for dir in ["/run/current-system/sw/share/X11/xkb/rules", "/etc/X11/xkb/rules", "/usr/share/X11/xkb/rules"] {
        candidates.push(PathBuf::from(dir));
    }
    if let Some(data_dirs) = std::env::var_os("XDG_DATA_DIRS") {
        for dir in std::env::split_paths(&data_dirs).filter(|dir| dir.is_absolute()) {
            candidates.push(dir.join("X11/xkb/rules"));
        }
    }
    candidates
        .into_iter()
        .find(|dir| dir.join("evdev.xml").is_file())
        .or_else(|| newest_in_store(Path::new("/nix/store")))
}

/// The rules directory of the newest `*-xkeyboard-config-<version>` in a
/// Nix store.
fn newest_in_store(store: &Path) -> Option<PathBuf> {
    let mut best: Option<(Vec<u32>, String, PathBuf)> = None;
    for entry in std::fs::read_dir(store).ok()?.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        let Some((_, version)) = name.split_once("-xkeyboard-config-") else {
            continue;
        };
        if version.is_empty() || !version.chars().all(|c| c.is_ascii_digit() || c == '.') {
            continue;
        }
        let numbers: Vec<u32> = version.split('.').map(|part| part.parse().unwrap_or(0)).collect();
        if best.as_ref().is_some_and(|(v, n, _)| (&numbers, name) <= (v, n.as_str())) {
            continue;
        }
        let rules = entry.path().join("share/X11/xkb/rules");
        if rules.join("evdev.xml").is_file() {
            best = Some((numbers, name.to_owned(), rules));
        }
    }
    best.map(|(_, _, rules)| rules)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aizome_apps::testutil::TempDir;

    /// Trimmed from xkeyboard-config 2.48's evdev.xml.
    const EVDEV: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE xkbConfigRegistry SYSTEM "xkb.dtd">
<xkbConfigRegistry version="1.1">
  <modelList>
    <model>
      <configItem>
        <name>pc105</name>
        <description>Generic 105-key PC</description>
        <vendor>Generic</vendor>
      </configItem>
    </model>
    <model>
      <configItem>
        <name>a4techKB21</name>
        <description>A4Tech KB-21</description>
        <vendor>A4Tech</vendor>
      </configItem>
    </model>
  </modelList>
  <layoutList>
    <layout>
      <configItem>
        <name>us</name>
        <!-- Keyboard indicator for English layouts -->
        <shortDescription>en</shortDescription>
        <description>English (US)</description>
        <countryList>
          <iso3166Id>US</iso3166Id>
        </countryList>
        <languageList>
          <iso639Id>eng</iso639Id>
        </languageList>
      </configItem>
      <variantList>
        <variant>
          <configItem>
            <name>euro</name>
            <description>English (US, euro on 5)</description>
          </configItem>
        </variant>
        <variant>
          <configItem>
            <name>intl</name>
            <description>English (US, intl., with dead keys)</description>
          </configItem>
        </variant>
        <variant>
          <configItem>
            <name>altgr-intl</name>
            <description>English (intl., with AltGr dead keys)</description>
            <languageList>
              <iso639Id>eng</iso639Id>
              <iso639Id>fra</iso639Id>
              <iso639Id>deu</iso639Id>
            </languageList>
          </configItem>
        </variant>
        <variant>
          <configItem>
            <name>colemak</name>
            <description>English (Colemak)</description>
          </configItem>
        </variant>
      </variantList>
    </layout>
    <layout>
      <configItem>
        <name>al</name>
        <!-- Keyboard indicator for Albanian layouts -->
        <shortDescription>sq</shortDescription>
        <description>Albanian</description>
        <countryList>
          <iso3166Id>AL</iso3166Id>
        </countryList>
        <languageList>
          <iso639Id>sqi</iso639Id>
        </languageList>
      </configItem>
      <variantList>
        <variant>
          <configItem>
            <name>plisi</name>
            <description>Albanian (Plisi)</description>
          </configItem>
        </variant>
      </variantList>
    </layout>
    <layout>
      <configItem>
        <name>de</name>
        <shortDescription>de</shortDescription>
        <description>German</description>
        <countryList>
          <iso3166Id>DE</iso3166Id>
        </countryList>
        <languageList>
          <iso639Id>deu</iso639Id>
        </languageList>
      </configItem>
      <variantList>
        <variant>
          <configItem>
            <name>nodeadkeys</name>
            <description>German (no dead keys)</description>
          </configItem>
        </variant>
        <variant>
          <configItem>
            <name>deadacute</name>
            <description>German (dead acute)</description>
          </configItem>
        </variant>
      </variantList>
    </layout>
    <layout>
      <configItem>
        <description>No name: skipped</description>
      </configItem>
    </layout>
    <layout>
      <configItem>
        <name>nodesc</name>
      </configItem>
    </layout>
  </layoutList>
  <optionList>
    <group allowMultipleSelection="true">
      <!-- The key combination used to switch between groups -->
      <configItem>
        <name>grp</name>
        <description>Switching to another layout</description>
      </configItem>
      <option>
        <configItem>
          <name>grp:alt_shift_toggle</name>
          <description>Alt+Shift</description>
        </configItem>
      </option>
    </group>
    <group allowMultipleSelection="true">
      <!-- Tweaking the position of the "Ctrl" key -->
      <configItem>
        <name>ctrl</name>
        <description>Ctrl position</description>
      </configItem>
      <option>
        <configItem>
          <name>ctrl:nocaps</name>
          <description>Caps Lock as Ctrl</description>
        </configItem>
      </option>
      <option>
        <configItem>
          <name>ctrl:swapcaps</name>
          <description>Swap Ctrl and Caps Lock</description>
        </configItem>
      </option>
    </group>
    <group allowMultipleSelection="false">
      <configItem>
        <name>caps</name>
        <description>Caps Lock behavior</description>
      </configItem>
      <option>
        <configItem>
          <name>caps:escape</name>
          <description>Make Caps Lock an additional Esc</description>
        </configItem>
      </option>
    </group>
  </optionList>
</xkbConfigRegistry>
"#;

    /// Trimmed from evdev.extras.xml.
    const EXTRAS: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<!DOCTYPE xkbConfigRegistry SYSTEM "xkb.dtd">
<xkbConfigRegistry>
  <modelList/>
  <layoutList>
    <layout>
      <configItem popularity="exotic">
        <name>ancient</name>
        <shortDescription>xx</shortDescription>
        <description>Ancient</description>
        <languageList>
          <iso639Id>got</iso639Id>
        </languageList>
      </configItem>
      <variantList>
        <variant>
          <configItem popularity="exotic">
            <name>got</name>
            <description>Gothic</description>
          </configItem>
        </variant>
      </variantList>
    </layout>
    <layout>
      <configItem popularity="exotic">
        <name>us</name>
        <shortDescription>en</shortDescription>
        <description>English (US)</description>
      </configItem>
      <variantList>
        <variant>
          <configItem popularity="exotic">
            <name>intl-unicode</name>
            <description>English (US, intl., AltGr Unicode combining)</description>
          </configItem>
        </variant>
        <variant>
          <configItem popularity="exotic">
            <name>intl</name>
            <description>Duplicate: ignored</description>
          </configItem>
        </variant>
      </variantList>
    </layout>
  </layoutList>
  <optionList>
    <group allowMultipleSelection="true">
      <configItem popularity="exotic">
        <name>lv3</name>
        <description>Key to choose the 3rd level</description>
      </configItem>
      <option>
        <configItem popularity="exotic">
          <name>lv3:4_switch_isolated</name>
          <description>Number key 4 when pressed in isolation</description>
        </configItem>
      </option>
    </group>
    <group allowMultipleSelection="false">
      <configItem popularity="exotic">
        <name>caps</name>
        <description>Caps Lock behavior</description>
      </configItem>
      <option>
        <configItem popularity="exotic">
          <name>caps:escape_shifted_compose</name>
          <description>Make Caps Lock an additional Esc, and Shift + Caps Lock is the Compose</description>
        </configItem>
      </option>
    </group>
  </optionList>
</xkbConfigRegistry>
"#;

    #[test]
    fn layouts_and_variants() {
        let data = KeyboardData::from_xml(EVDEV).unwrap();
        let names: Vec<&str> = data.layouts.iter().map(|l| l.name.as_str()).collect();
        // Sorted by description; the nameless one skipped.
        assert_eq!(names, ["al", "us", "de", "nodesc"]);
        let us = data.layout("us").unwrap();
        assert_eq!(us.short_description.as_deref(), Some("en"));
        assert_eq!(us.languages, ["eng"]);
        assert_eq!(us.countries, ["US"]);
        assert!(!us.extra);
        let variants: Vec<&str> = us.variants.iter().map(|v| v.name.as_str()).collect();
        assert_eq!(variants, ["colemak", "altgr-intl", "euro", "intl"]);
        assert_eq!(data.variant("us", "altgr-intl").unwrap().languages, ["eng", "fra", "deu"]);
        // Inherited from the layout.
        assert_eq!(data.variant("us", "intl").unwrap().languages, ["eng"]);
        assert_eq!(data.layout("nodesc").unwrap().description, "nodesc");
        assert!(data.variant("us", "nope").is_none());
        assert!(data.variant("zz", "intl").is_none());
        assert_eq!(data.models.len(), 2);
        assert_eq!(data.models[0].vendor.as_deref(), Some("Generic"));
    }

    #[test]
    fn descriptions() {
        let data = KeyboardData::from_xml(EVDEV).unwrap();
        assert_eq!(data.describe("us", "intl"), "English (US, intl., with dead keys)");
        assert_eq!(data.describe("us", ""), "English (US)");
        assert_eq!(data.describe("de", "nodeadkeys"), "German (no dead keys)");
        assert_eq!(data.describe("de", "unknown"), "German (unknown)");
        assert_eq!(data.describe("xx", ""), "xx");
        assert_eq!(data.describe("xx", "yy"), "xx (yy)");
        let choices = data.choices();
        assert_eq!(choices.len(), 4 + 4 + 1 + 2);
        assert_eq!(choices[0], LayoutChoice { layout: "al", variant: None, description: "Albanian", extra: false });
        assert_eq!(choices[1].variant, Some("plisi"));
        assert!(choices.windows(2).all(|w| w[0].description.to_lowercase() <= w[1].description.to_lowercase()));
    }

    #[test]
    fn options() {
        let data = KeyboardData::from_xml(EVDEV).unwrap();
        let groups: Vec<&str> = data.option_groups.iter().map(|g| g.name.as_str()).collect();
        assert_eq!(groups, ["grp", "ctrl", "caps"]);
        let ctrl = data.option_group("ctrl").unwrap();
        assert!(ctrl.allow_multiple_selection);
        assert_eq!(ctrl.description, "Ctrl position");
        assert!(!data.option_group("caps").unwrap().allow_multiple_selection);
        assert_eq!(data.option("ctrl:nocaps").unwrap().description, "Caps Lock as Ctrl");
        assert!(data.option("ctrl:nope").is_none());
    }

    #[test]
    fn extras_merge_and_are_marked() {
        let mut data = KeyboardData::from_xml(EVDEV).unwrap();
        data.merge_extras_xml(EXTRAS).unwrap();
        let ancient = data.layout("ancient").unwrap();
        assert!(ancient.extra && ancient.variants[0].extra);
        assert_eq!(ancient.variants[0].languages, ["got"]);
        let us = data.layout("us").unwrap();
        assert!(!us.extra);
        assert_eq!(us.variants.len(), 5);
        assert!(data.variant("us", "intl-unicode").unwrap().extra);
        assert!(!data.variant("us", "intl").unwrap().extra);
        assert_eq!(data.describe("us", "intl"), "English (US, intl., with dead keys)");
        assert!(data.option_group("lv3").unwrap().extra);
        let caps = data.option_group("caps").unwrap();
        assert!(!caps.extra);
        assert_eq!(caps.options.len(), 2);
        assert!(data.option("caps:escape_shifted_compose").unwrap().extra);
        // Still sorted.
        let names: Vec<&str> = data.layouts.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(names, ["al", "ancient", "us", "de", "nodesc"]);
    }

    #[test]
    fn bad_input_is_an_error_not_a_panic() {
        assert!(KeyboardData::from_xml("").is_err());
        assert!(KeyboardData::from_xml("<notXkb/>").is_err());
        assert!(KeyboardData::from_xml("<xkbConfigRegistry><layoutList>").is_err());
        let empty = KeyboardData::from_xml("<xkbConfigRegistry/>").unwrap();
        assert!(empty.layouts.is_empty() && empty.option_groups.is_empty());
        let odd = KeyboardData::from_xml(
            "<xkbConfigRegistry><layoutList><layout/><layout><configItem><name> sp  aced </name>\
             <description>A <!-- c --> B</description></configItem><variantList><variant/></variantList></layout>\
             </layoutList><optionList><group><option/></group></optionList></xkbConfigRegistry>",
        )
        .unwrap();
        assert_eq!(odd.layouts.len(), 1);
        assert_eq!(odd.layouts[0].name, "sp aced");
        assert_eq!(odd.layouts[0].description, "A B");
        assert!(odd.layouts[0].variants.is_empty());
        assert!(odd.option_groups.is_empty());
    }

    #[test]
    fn files_and_the_newest_store_path() {
        let tmp = TempDir::new("xkb");
        tmp.write("rules/evdev.xml", EVDEV);
        tmp.write("rules/evdev.extras.xml", EXTRAS);
        let data = KeyboardData::load_from(&tmp.path().join("rules"), true).unwrap();
        assert!(data.layout("ancient").is_some());
        assert_eq!(data.source.as_deref(), Some(tmp.path().join("rules").as_path()));
        let base = KeyboardData::load_from(&tmp.path().join("rules"), false).unwrap();
        assert!(base.layout("ancient").is_none());
        assert!(matches!(KeyboardData::load_from(&tmp.path().join("nope"), false), Err(Error::NotFound(_))));

        let store = tmp.path().join("store");
        for name in [
            "aaa-xkeyboard-config-2.9",
            "bbb-xkeyboard-config-2.48",
            "ccc-xkeyboard-config-2.48",
            "ddd-xkeyboard-config-2.50.drv",
            "eee-xkeyboard-config-2.51-man",
        ] {
            tmp.write(&format!("store/{name}/share/X11/xkb/rules/evdev.xml"), EVDEV);
        }
        tmp.write("store/fff-xkeyboard-config-3.0/share/README", "no rules");
        assert_eq!(newest_in_store(&store), Some(store.join("ccc-xkeyboard-config-2.48/share/X11/xkb/rules")));
        assert_eq!(newest_in_store(&tmp.path().join("no-store")), None);
    }

    #[test]
    #[ignore = "reads this machine's xkeyboard-config"]
    fn live_registry() {
        let started = std::time::Instant::now();
        let dir = find_rules_dir().expect("an evdev.xml somewhere");
        let found = started.elapsed();
        let data = KeyboardData::load_from(&dir, true).unwrap();
        println!(
            "{}: {} layouts, {} variants, {} option groups, {} models (found in {found:?}, total {:?})",
            dir.display(),
            data.layouts.len(),
            data.layouts.iter().map(|l| l.variants.len()).sum::<usize>(),
            data.option_groups.len(),
            data.models.len(),
            started.elapsed()
        );
        assert!(data.layout("us").is_some());
        let de = data.layout("de").unwrap();
        assert!(de.variants.iter().any(|v| v.name == "nodeadkeys"));
        assert_eq!(data.describe("us", "intl"), "English (US, intl., with dead keys)");
        assert_eq!(data.option("ctrl:nocaps").unwrap().description, "Caps Lock as Ctrl");
        assert!(data.layouts.iter().any(|l| l.extra));
        let plain = KeyboardData::load().unwrap();
        assert!(plain.layouts.iter().all(|l| !l.extra));
    }
}
