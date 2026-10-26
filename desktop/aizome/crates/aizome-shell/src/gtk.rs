//! GTK and libadwaita apps follow Aizome's look: the theme and the focus
//! colour are written to dconf, where the session's `aizome` profile keeps
//! its defaults under the user's own values. libadwaita (and the settings
//! portal, which relays it) reads `color-scheme` and `accent-color`; GTK 3
//! reads the theme's name; both read the high-contrast switch.

use aizome_theme::{FocusColor, ThemeName};

/// What GTK is told.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GtkLook {
    /// `org.gnome.desktop.interface color-scheme`.
    pub color_scheme: &'static str,
    /// `org.gnome.desktop.interface gtk-theme`, for GTK 3.
    pub gtk_theme: &'static str,
    /// `org.gnome.desktop.interface accent-color`: GNOME's nearest.
    pub accent: &'static str,
    /// `org.gnome.desktop.a11y.interface high-contrast`.
    pub high_contrast: bool,
}

/// GTK's look for an Aizome theme and focus colour.
pub fn look(theme: ThemeName, focus: FocusColor) -> GtkLook {
    let (color_scheme, gtk_theme, high_contrast) = match theme {
        ThemeName::Day => ("prefer-light", "Adwaita", false),
        ThemeName::Night => ("prefer-dark", "Adwaita-dark", false),
        // High Contrast is light on black.
        ThemeName::HighContrast => ("prefer-dark", "HighContrastInverse", true),
    };
    let accent = match focus {
        FocusColor::Amber => "yellow",
        FocusColor::Cyan => "teal",
        FocusColor::Rose => "pink",
        FocusColor::Lime => "green",
    };
    GtkLook { color_scheme, gtk_theme, accent, high_contrast }
}

/// The `dconf write` arguments that tell GTK `look`.
pub fn writes(look: &GtkLook) -> Vec<[String; 2]> {
    let s = |v: &str| format!("'{v}'");
    vec![
        ["/org/gnome/desktop/interface/color-scheme".into(), s(look.color_scheme)],
        ["/org/gnome/desktop/interface/gtk-theme".into(), s(look.gtk_theme)],
        ["/org/gnome/desktop/interface/accent-color".into(), s(look.accent)],
        ["/org/gnome/desktop/a11y/interface/high-contrast".into(), look.high_contrast.to_string()],
    ]
}

/// Tell GTK, on a thread of its own (dconf may be slow to start).
pub fn tell(look: GtkLook) {
    std::thread::spawn(move || {
        for [key, value] in writes(&look) {
            let status = std::process::Command::new("dconf")
                .args(["write", &key, &value])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();
            if let Err(e) = status {
                log::warn!("dconf write {key}: {e}");
                return;
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn day_is_light_for_libadwaita_and_gtk3() {
        let l = look(ThemeName::Day, FocusColor::Rose);
        assert_eq!(l.color_scheme, "prefer-light");
        assert_eq!(l.gtk_theme, "Adwaita");
        assert_eq!(l.accent, "pink");
        assert!(!l.high_contrast);
    }

    #[test]
    fn night_and_high_contrast_are_dark() {
        assert_eq!(look(ThemeName::Night, FocusColor::Amber).gtk_theme, "Adwaita-dark");
        let hc = look(ThemeName::HighContrast, FocusColor::Cyan);
        assert_eq!(hc.color_scheme, "prefer-dark");
        assert!(hc.high_contrast);
        assert_eq!(hc.accent, "teal");
    }

    #[test]
    fn values_are_gvariant_text() {
        let w = writes(&look(ThemeName::Night, FocusColor::Lime));
        assert!(w.contains(&["/org/gnome/desktop/interface/color-scheme".into(), "'prefer-dark'".into()]));
        assert!(w.contains(&["/org/gnome/desktop/interface/accent-color".into(), "'green'".into()]));
        assert!(w.contains(&["/org/gnome/desktop/a11y/interface/high-contrast".into(), "false".into()]));
    }
}
