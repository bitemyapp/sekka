//! What each app is made with, as the Toolchest and the palette mark it
//! beside its name.

use aizome_kit::theme::Role;
use aizome_kit::{Icon, ItemMark};

/// The toolkit an app draws its windows with.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Toolkit {
    /// Aizome's own kit.
    Aizome,
    /// GTK 4 with libadwaita.
    Adwaita,
    Gtk4,
    Gtk3,
    Qt6,
    Qt5,
    Electron,
    /// Firefox's engine.
    Gecko,
    Chromium,
    /// Chromium Embedded (Spotify, Steam).
    Cef,
    /// Zed's toolkit (Zed, Kakehashi).
    Gpui,
    /// No toolkit: it draws its own windows (foot, kitty, mpv).
    Own,
    /// An X11 program, through XWayland.
    X11,
    /// A program for the terminal.
    Terminal,
    Java,
}

/// How the marks show.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MarkStyle {
    #[default]
    Off,
    /// A word in a small tag.
    Tag,
    /// A glyph (Aizome's own apps: the mark).
    Glyph,
}

impl Toolkit {
    /// Its name in full, for the palette's detail pane.
    pub fn name(self) -> &'static str {
        match self {
            Toolkit::Aizome => "Aizome's own kit",
            Toolkit::Adwaita => "GTK 4 with libadwaita",
            Toolkit::Gtk4 => "GTK 4",
            Toolkit::Gtk3 => "GTK 3",
            Toolkit::Qt6 => "Qt 6",
            Toolkit::Qt5 => "Qt 5",
            Toolkit::Electron => "Electron",
            Toolkit::Gecko => "Gecko, Firefox's engine",
            Toolkit::Chromium => "Chromium",
            Toolkit::Cef => "Chromium Embedded",
            Toolkit::Gpui => "GPUI, Zed's toolkit",
            Toolkit::Own => "Its own drawing, on Wayland",
            Toolkit::X11 => "X11, through XWayland",
            Toolkit::Terminal => "A terminal program",
            Toolkit::Java => "Java",
        }
    }

    /// The word its tag shows.
    pub fn word(self) -> &'static str {
        match self {
            Toolkit::Aizome => "Aizome",
            Toolkit::Adwaita | Toolkit::Gtk4 => "GTK 4",
            Toolkit::Gtk3 => "GTK 3",
            Toolkit::Qt6 => "Qt 6",
            Toolkit::Qt5 => "Qt 5",
            Toolkit::Electron => "Electron",
            Toolkit::Gecko => "Gecko",
            Toolkit::Chromium => "Chromium",
            Toolkit::Cef => "CEF",
            Toolkit::Gpui => "GPUI",
            Toolkit::Own => "Own",
            Toolkit::X11 => "X11",
            Toolkit::Terminal => "TUI",
            Toolkit::Java => "Java",
        }
    }

    /// Its glyph (Aizome's apps show the mark instead).
    pub fn glyph(self) -> Icon {
        match self {
            Toolkit::Aizome | Toolkit::Adwaita | Toolkit::Gtk4 | Toolkit::Gtk3 => Icon::Box,
            Toolkit::Qt6 | Toolkit::Qt5 => Icon::ToolkitQt,
            Toolkit::Electron => Icon::ToolkitAtom,
            Toolkit::Gecko => Icon::Flame,
            Toolkit::Chromium | Toolkit::Cef => Icon::CircleDot,
            Toolkit::Gpui => Icon::Zap,
            Toolkit::Own => Icon::Paintbrush,
            Toolkit::X11 => Icon::ToolkitX11,
            Toolkit::Terminal => Icon::SquareTerminal,
            Toolkit::Java => Icon::Coffee,
        }
    }

    /// Its colour: one per family, the uncommon ones quiet.
    pub fn role(self) -> Role {
        match self {
            Toolkit::Aizome => Role::Accent,
            Toolkit::Adwaita | Toolkit::Gtk4 | Toolkit::Gtk3 => Role::Cyan,
            Toolkit::Qt6 | Toolkit::Qt5 => Role::Success,
            Toolkit::Electron => Role::Violet,
            Toolkit::Gecko | Toolkit::Chromium | Toolkit::Cef => Role::Warning,
            Toolkit::X11 => Role::Danger,
            Toolkit::Gpui | Toolkit::Own | Toolkit::Terminal | Toolkit::Java => Role::Text3,
        }
    }

    /// Its mark in `style`.
    pub fn mark(self, style: MarkStyle) -> Option<ItemMark> {
        match style {
            MarkStyle::Off => None,
            MarkStyle::Tag => Some(ItemMark::Tag(self.word().into(), self.role())),
            MarkStyle::Glyph if self == Toolkit::Aizome => Some(ItemMark::Brand),
            MarkStyle::Glyph => Some(ItemMark::Glyph(self.glyph(), self.role())),
        }
    }
}
