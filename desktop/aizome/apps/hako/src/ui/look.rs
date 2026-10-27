//! How things look: the icon and dye of each kind of item and place.

use aizome_kit::{Color, Icon, Role, Theme};

use crate::model::entry::{Entry, Kind};
use crate::model::places::PlaceKind;

/// An item's icon, and the role its colour comes from.
pub fn entry_icon(e: &Entry) -> (Icon, Role) {
    match e.kind {
        Kind::Folder => (Icon::Folder, Role::Accent),
        Kind::Link { to_folder: true, .. } => (Icon::FolderSymlink, Role::Accent),
        Kind::Link { broken: true, .. } => (Icon::FileSymlink, Role::Faint),
        Kind::Special => (Icon::File, Role::Faint),
        _ => mime_icon(&e.mime, e.display.as_str()),
    }
}

/// A file type's icon.
pub fn mime_icon(mime: &str, name: &str) -> (Icon, Role) {
    let (major, minor) = mime.split_once('/').unwrap_or((mime, ""));
    let lower = name.to_ascii_lowercase();
    if crate::fsops::archive::is_archive(&lower) && !lower.ends_with(".iso") {
        return (Icon::FileArchive, Role::Warning);
    }
    match major {
        "image" => (Icon::FileImage, Role::Violet),
        "video" => (Icon::FilePlay, Role::Violet),
        "audio" => (Icon::FileMusic, Role::Cyan),
        "font" => (Icon::FileType, Role::Text2),
        "text" if is_code(minor) => (Icon::FileCode, Role::Cyan),
        "text" if minor == "csv" => (Icon::FileSpreadsheet, Role::Success),
        "text" => (Icon::FileText, Role::Success),
        "inode" if minor == "directory" => (Icon::Folder, Role::Accent),
        _ => match minor {
            "pdf" => (Icon::FileText, Role::Danger),
            "x-cd-image" | "x-iso9660-image" | "x-raw-disk-image" => (Icon::Disc, Role::Violet),
            "json" | "xml" | "javascript" | "toml" | "x-yaml" | "yaml" | "x-shellscript" | "x-perl" | "x-ruby"
            | "sql" | "x-typescript" | "wasm" => (Icon::FileCode, Role::Cyan),
            "x-executable" | "x-sharedlib" | "x-pie-executable" => (Icon::FileTerminal, Role::Text2),
            "x-desktop" => (Icon::AppWindow, Role::Text2),
            m if m.contains("spreadsheet") || m.contains("excel") => (Icon::FileSpreadsheet, Role::Success),
            m if m.contains("zip") || m.contains("compressed") || m.contains("tar") || m.contains("archive") => {
                (Icon::FileArchive, Role::Warning)
            }
            m if m.contains("document") || m.contains("word") || m.contains("opendocument") => {
                (Icon::FileText, Role::Accent)
            }
            "x-zerosize" => (Icon::File, Role::Text3),
            _ => (Icon::File, Role::Text2),
        },
    }
}

fn is_code(minor: &str) -> bool {
    let m = minor.trim_start_matches("x-");
    matches!(
        m,
        "rust"
            | "python"
            | "csrc"
            | "chdr"
            | "c++src"
            | "c++hdr"
            | "java"
            | "go"
            | "javascript"
            | "nix"
            | "makefile"
            | "cmake"
            | "shellscript"
            | "lua"
            | "haskell"
            | "ocaml"
            | "zig"
            | "swift"
            | "kotlin"
            | "scala"
            | "ruby"
            | "perl"
            | "php"
            | "css"
            | "html"
            | "scss"
            | "typescript"
            | "vala"
            | "csharp"
            | "elixir"
            | "erlang"
            | "dart"
            | "patch"
            | "diff"
            | "toml"
            | "yaml"
            | "sql"
            | "tex"
            | "meson"
    )
}

/// A place's icon and dye.
pub fn place_icon(kind: PlaceKind) -> (Icon, Role) {
    match kind {
        PlaceKind::Home => (Icon::House, Role::Accent),
        PlaceKind::Desktop => (Icon::Monitor, Role::Text2),
        PlaceKind::Documents => (Icon::FileText, Role::Text2),
        PlaceKind::Downloads => (Icon::Download, Role::Text2),
        PlaceKind::Pictures => (Icon::Image, Role::Text2),
        PlaceKind::Music => (Icon::EighthNotes, Role::Text2),
        PlaceKind::Videos => (Icon::Film, Role::Text2),
        PlaceKind::Folder => (Icon::Folder, Role::Text2),
        PlaceKind::Desk => (Icon::FolderOpen, Role::Warning),
        PlaceKind::Disk => (Icon::HardDrive, Role::Text2),
        PlaceKind::Volume => (Icon::Usb, Role::Text2),
        PlaceKind::Image => (Icon::Disc, Role::Violet),
        PlaceKind::Trash => (Icon::Trash, Role::Text2),
        PlaceKind::Recent => (Icon::Clock, Role::Text2),
    }
}

/// The colour of a role in the theme, with a selected row's on top.
pub fn tint(theme: &Theme, role: Role, on_selection: bool) -> Color {
    if on_selection { theme.on_selection() } else { theme.role(role) }
}
