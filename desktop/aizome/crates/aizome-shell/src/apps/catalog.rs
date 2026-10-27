//! The installed apps as the shell shows them, over `aizome-sys`'s
//! desktop-entry index: the Toolchest's categories, the Deskbar's names and
//! glyphs, icon files.

use aizome_kit::Icon;
use aizome_sys::apps::{App, AppIndex};

use crate::look::Hue;

/// The Toolchest's categories, in its order.
pub const CATEGORIES: [&str; 10] =
    ["Accessories", "Development", "Games", "Graphics", "Internet", "Office", "Media", "System", "Utilities", "Other"];

/// Whether it's one of Aizome's own apps.
pub fn is_aizome(app: &App) -> bool {
    app.app_id().starts_with("org.aizome.")
}

/// An app's place in the Toolchest's Applications, from its categories.
pub fn category(categories: &[String]) -> &'static str {
    let has = |c: &str| categories.iter().any(|x| x == c);
    if has("Development") {
        "Development"
    } else if has("Game") {
        "Games"
    } else if has("Utility") && ["TextEditor", "Calculator", "Clock", "Accessibility", "Viewer"].iter().any(|c| has(c))
    {
        "Accessories"
    } else if has("Graphics") {
        "Graphics"
    } else if has("Network") {
        "Internet"
    } else if has("Office") {
        "Office"
    } else if has("AudioVideo") || has("Audio") || has("Video") {
        "Media"
    } else if has("System") || has("Settings") {
        "System"
    } else if has("Utility") {
        "Utilities"
    } else {
        "Other"
    }
}

/// The apps shown in menus, by category in the Toolchest's order, Aizome's
/// first in each and then by name.
pub fn by_category(apps: &[&App]) -> Vec<(&'static str, Vec<App>)> {
    CATEGORIES
        .iter()
        .map(|cat| {
            let mut in_cat: Vec<App> =
                apps.iter().filter(|a| category(&a.categories) == *cat).map(|a| (*a).clone()).collect();
            in_cat.sort_by(|a, b| {
                is_aizome(b).cmp(&is_aizome(a)).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            });
            (*cat, in_cat)
        })
        .filter(|(_, apps)| !apps.is_empty())
        .collect()
}

/// What the shell calls a running app: its desktop entry's name, the name
/// it said hello with, or its id made readable (`org.gnome.Nautilus` is
/// Nautilus).
pub fn display_name(index: Option<&AppIndex>, hello_name: Option<&str>, app_id: &str) -> String {
    if let Some(app) = index.and_then(|i| i.match_window(app_id)) {
        return app.name.clone();
    }
    if let Some(n) = hello_name.filter(|n| !n.is_empty()) {
        return n.to_owned();
    }
    let last = app_id.rsplit('.').next().unwrap_or(app_id);
    let mut c = last.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => "App".into(),
    }
}

/// The glyph and colour an app shows with where the shell knows one:
/// Aizome's apps by kind, and the common ones by name.
pub fn glyph_for(app_id: &str) -> Option<(Icon, Hue)> {
    let id = app_id.to_ascii_lowercase();
    let id = id.strip_suffix(".desktop").unwrap_or(&id);
    let last = id.rsplit('.').next().unwrap_or(id);
    Some(match last {
        "settei" => (Icon::Settings, Hue::Text2),
        "yukimi" => (Icon::Package, Hue::Blue),
        "hako" => (Icon::Folder, Hue::Blue),
        "sumi" => (Icon::Code, Hue::Violet),
        "fusen" => (Icon::StickyNote, Hue::Amber),
        "koyomi" => (Icon::CalendarDays, Hue::Red),
        "tokei" => (Icon::Clock, Hue::Cyan),
        "soroban" => (Icon::Calculator, Hue::Green),
        "tenki" => (Icon::CloudSun, Hue::Cyan),
        "tansu" => (Icon::HardDrive, Hue::Text2),
        "kura" => (Icon::ChartPie, Hue::Violet),
        "myaku" => (Icon::Activity, Hue::Red),
        "shashin" => (Icon::Image, Hue::Violet),
        "annai" => (Icon::Compass, Hue::Amber),
        "foot" | "footclient" | "alacritty" | "kitty" | "wezterm" | "ghostty" | "konsole" | "terminal" | "xterm" => {
            (Icon::SquareTerminal, Hue::Green)
        }
        "firefox" | "chromium" | "chromium-browser" | "google-chrome" | "brave-browser" | "epiphany" | "librewolf" => {
            (Icon::Globe, Hue::Cyan)
        }
        "thunderbird" | "evolution" | "geary" | "mail" => (Icon::Mail, Hue::Amber),
        "nautilus" | "thunar" | "dolphin" | "pcmanfm" | "nemo" => (Icon::Folder, Hue::Blue),
        "code" | "codium" | "zed" | "helix" | "nvim" | "emacs" | "gedit" => (Icon::Code, Hue::Violet),
        "mpv" | "vlc" | "totem" => (Icon::Film, Hue::Red),
        "gimp" | "inkscape" | "krita" => (Icon::Paintbrush, Hue::Violet),
        "imv" | "eog" | "loupe" => (Icon::Image, Hue::Violet),
        _ => return None,
    })
}

/// A glyph for an app the shell knows nothing about, from its categories.
pub fn glyph_by_category(categories: &[String]) -> Icon {
    match category(categories) {
        "Accessories" => Icon::Wrench,
        "Development" => Icon::Code,
        "Graphics" => Icon::Paintbrush,
        "Internet" => Icon::Globe,
        "Office" => Icon::FileText,
        "Media" => Icon::Film,
        "System" => Icon::Settings2,
        "Utilities" => Icon::Wrench,
        _ => Icon::AppWindow,
    }
}

/// The Toolchest's glyph for a category.
pub fn category_glyph(cat: &str) -> Icon {
    match cat {
        "Accessories" => Icon::Wrench,
        "Development" => Icon::CodeXml,
        "Graphics" => Icon::Palette,
        "Internet" => Icon::Globe,
        "Office" => Icon::FileText,
        "Media" => Icon::Film,
        "System" => Icon::Settings2,
        "Utilities" => Icon::Hammer,
        _ => Icon::Box,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cats(s: &str) -> Vec<String> {
        s.split(';').map(String::from).collect()
    }

    #[test]
    fn categories_map_to_the_toolchests() {
        assert_eq!(category(&cats("Utility;Calculator")), "Accessories");
        assert_eq!(category(&cats("Utility;Archiving")), "Utilities");
        assert_eq!(category(&cats("Network;WebBrowser")), "Internet");
        assert_eq!(category(&cats("System;TerminalEmulator")), "System");
        assert_eq!(category(&cats("Game")), "Games");
    }

    #[test]
    fn development_editors_belong_with_development_tools() {
        // Both Zed and VS Code publish these categories. Their generic
        // text-editor role must not hide the more specific IDE role.
        assert_eq!(category(&cats("Utility;TextEditor;Development;IDE")), "Development");
        assert_eq!(category(&cats("Utility;TextEditor")), "Accessories");
    }

    #[test]
    fn steam_desktop_entries_appear_in_games() {
        let app = aizome_sys::apps::DesktopEntry::parse(
            "[Desktop Entry]\nType=Application\nName=Example Game\nExec=steam steam://rungameid/123\nCategories=Game;\n",
            "Example Game.desktop",
            &aizome_sys::apps::Locale::untranslated(),
        ).unwrap();
        let groups = by_category(&[&app]);
        assert_eq!(groups[0].0, "Games");
        assert_eq!(groups[0].1[0].exec.as_deref(), Some("steam steam://rungameid/123"));
    }

    #[test]
    fn names_fall_back_to_the_id() {
        assert_eq!(display_name(None, None, "org.gnome.Nautilus"), "Nautilus");
        assert_eq!(display_name(None, Some("Sumi"), "org.aizome.Sumi"), "Sumi");
        assert_eq!(display_name(None, None, "foot"), "Foot");
    }

    #[test]
    fn known_apps_have_glyphs() {
        assert_eq!(glyph_for("org.aizome.Sumi").map(|g| g.0), Some(Icon::Code));
        assert_eq!(glyph_for("foot").map(|g| g.0), Some(Icon::SquareTerminal));
        assert_eq!(glyph_for("firefox.desktop").map(|g| g.0), Some(Icon::Globe));
        assert!(glyph_for("com.example.Unknown").is_none());
    }
}
