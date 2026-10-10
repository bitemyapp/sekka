// SPDX-License-Identifier: GPL-3.0-or-later
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

/// A nonempty, unique, compatible selection whose default is a member.
/// The order is retained so configuration output remains stable.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DesktopSelection {
    selected: Vec<Desktop>,
    default: Desktop,
}
impl DesktopSelection {
    pub fn parse(selected: Vec<Desktop>, default: Desktop) -> Result<Self> {
        ensure!(
            !selected.is_empty() && selected.len() <= Desktop::ALL.len(),
            "Select at least one desktop environment"
        );
        for (index, desktop) in selected.iter().enumerate() {
            ensure!(
                !selected[..index].contains(desktop),
                "Duplicate desktop selection"
            );
        }
        ensure!(
            selected.contains(&default),
            "The default session must be a selected desktop"
        );
        Ok(Self { selected, default })
    }
    pub fn selected(&self) -> &[Desktop] {
        &self.selected
    }
    pub fn default(&self) -> Desktop {
        self.default
    }
    pub fn contains(&self, desktop: Desktop) -> bool {
        self.selected.contains(&desktop)
    }
    pub(crate) fn into_raw(self) -> (Vec<Desktop>, Desktop) {
        (self.selected, self.default)
    }
}

/// The desktops the installer offers: the Wayland desktops, and Xfce as the
/// one traditional X11 desktop. MATE, LXQt and Cinnamon are no longer
/// offered (they duplicated Xfce's role and caused most cross-desktop
/// interference); installed systems that have them keep working, since the
/// NixOS module still accepts them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Desktop {
    Plasma,
    Gnome,
    Xfce,
    Hyprland,
    /// Tatami: keyboard-driven Hyprland inspired by Omarchy, as its own login
    /// session. Requests and configurations from before the rename say
    /// "omarchy".
    #[serde(alias = "omarchy")]
    Tatami,
}
impl Desktop {
    pub const ALL: [Self; 5] = [
        Self::Plasma,
        Self::Gnome,
        Self::Xfce,
        Self::Hyprland,
        Self::Tatami,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Plasma => "KDE Plasma",
            Self::Gnome => "GNOME",
            Self::Xfce => "Xfce",
            Self::Hyprland => "Hyprland",
            Self::Tatami => "Tatami",
        }
    }
    pub fn description(self) -> &'static str {
        match self {
            Self::Plasma => "Full-featured and familiar, with deep customization.",
            Self::Gnome => "Focused, modern workflow built around the Activities overview.",
            Self::Xfce => "Lightweight and traditional; easy on older hardware.",
            Self::Hyprland => {
                "Dynamic tiling Wayland compositor with its upstream default configuration."
            }
            Self::Tatami => {
                "Keyboard-driven Hyprland inspired by Omarchy: a Tokyo Night look, menus for everything and Omarchy's key bindings."
            }
        }
    }
    /// Value of `calamares.desktops` in the installed configuration.
    pub fn id(self) -> &'static str {
        match self {
            Self::Plasma => "plasma",
            Self::Gnome => "gnome",
            Self::Xfce => "xfce",
            Self::Hyprland => "hyprland",
            Self::Tatami => "tatami",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ids_match_serde_names_and_hyprland_flavors_combine() {
        for desktop in Desktop::ALL {
            assert_eq!(
                serde_json::to_string(&desktop).unwrap(),
                format!("\"{}\"", desktop.id())
            );
        }
        let both =
            DesktopSelection::parse(vec![Desktop::Hyprland, Desktop::Tatami], Desktop::Tatami)
                .unwrap();
        assert_eq!(both.default(), Desktop::Tatami);
        // Requests naming desktops the installer no longer offers are refused.
        assert!(serde_json::from_str::<Desktop>("\"mate\"").is_err());
        assert!(DesktopSelection::parse(vec![Desktop::Hyprland], Desktop::Plasma).is_err());
    }
}
