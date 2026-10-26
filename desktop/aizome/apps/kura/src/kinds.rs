//! What kind of thing a file is, from its name: enough to say "38 GB of
//! video" without opening anything.

use serde::{Deserialize, Serialize};

/// A kind of file, as the index names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FileKind {
    Video,
    Audio,
    Image,
    Document,
    Archive,
    DiskImage,
    Code,
    /// Compiled objects, libraries, executables, build artefacts.
    Binary,
    Database,
    Font,
    Log,
    /// Version control's own data (`.git/objects` packs and the like).
    VersionControl,
    Other,
}

impl FileKind {
    pub const ALL: [FileKind; 13] = [
        FileKind::Video,
        FileKind::Audio,
        FileKind::Image,
        FileKind::Document,
        FileKind::Archive,
        FileKind::DiskImage,
        FileKind::Code,
        FileKind::Binary,
        FileKind::Database,
        FileKind::Font,
        FileKind::Log,
        FileKind::VersionControl,
        FileKind::Other,
    ];

    /// Its name in the index (`disk-image`).
    pub fn id(self) -> &'static str {
        match self {
            FileKind::Video => "video",
            FileKind::Audio => "audio",
            FileKind::Image => "image",
            FileKind::Document => "document",
            FileKind::Archive => "archive",
            FileKind::DiskImage => "disk-image",
            FileKind::Code => "code",
            FileKind::Binary => "binary",
            FileKind::Database => "database",
            FileKind::Font => "font",
            FileKind::Log => "log",
            FileKind::VersionControl => "version-control",
            FileKind::Other => "other",
        }
    }

    /// Its name for people.
    pub fn title(self) -> &'static str {
        match self {
            FileKind::Video => "Video",
            FileKind::Audio => "Audio",
            FileKind::Image => "Images",
            FileKind::Document => "Documents",
            FileKind::Archive => "Archives",
            FileKind::DiskImage => "Disk images",
            FileKind::Code => "Source code",
            FileKind::Binary => "Programs and libraries",
            FileKind::Database => "Databases",
            FileKind::Font => "Fonts",
            FileKind::Log => "Logs",
            FileKind::VersionControl => "Version control",
            FileKind::Other => "Other",
        }
    }
}

/// The kind of a file named `name`, inside a `.git` folder or not.
pub fn classify(name: &[u8], in_git: bool) -> FileKind {
    if in_git {
        return FileKind::VersionControl;
    }
    let name = String::from_utf8_lossy(name).to_ascii_lowercase();
    // LevelDB's `000123.log` is its write-ahead data.
    if name.strip_suffix(".log").is_some_and(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())) {
        return FileKind::Database;
    }
    if name.ends_with(".log")
        || name.contains(".log.")
        || name == "syslog"
        || name.ends_with(".journal")
        || name.ends_with(".journal~")
    {
        return FileKind::Log;
    }
    let ext = match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => ext,
        _ => return FileKind::Other,
    };
    match ext {
        "mp4" | "mkv" | "mov" | "avi" | "webm" | "m4v" | "wmv" | "flv" | "mpg" | "mpeg" | "mts" | "m2ts" | "3gp"
        | "ogv" => FileKind::Video,
        "mp3" | "flac" | "ogg" | "opus" | "wav" | "m4a" | "aac" | "wma" | "aiff" | "aif" | "alac" | "mid" | "midi" => {
            FileKind::Audio
        }
        "jpg" | "jpeg" | "png" | "gif" | "webp" | "avif" | "heic" | "heif" | "tif" | "tiff" | "bmp" | "svg" | "raw"
        | "cr2" | "cr3" | "nef" | "arw" | "dng" | "psd" | "xcf" | "kra" | "ico" | "jxl" | "exr" => FileKind::Image,
        "pdf" | "epub" | "mobi" | "djvu" | "doc" | "docx" | "odt" | "rtf" | "xls" | "xlsx" | "ods" | "ppt" | "pptx"
        | "odp" | "txt" | "md" | "tex" | "csv" | "pages" | "key" | "numbers" => FileKind::Document,
        "zip" | "tar" | "gz" | "tgz" | "bz2" | "xz" | "zst" | "7z" | "rar" | "lz" | "lz4" | "lzma" | "zstd" | "deb"
        | "rpm" | "apk" | "jar" | "whl" | "crate" | "nar" | "cab" => FileKind::Archive,
        "iso" | "img" | "qcow2" | "qcow" | "vdi" | "vmdk" | "vhd" | "vhdx" | "dmg" | "appimage" | "ova"
        | "squashfs" | "wim" => FileKind::DiskImage,
        "rs" | "c" | "h" | "cc" | "cpp" | "hpp" | "cxx" | "py" | "js" | "mjs" | "cjs" | "ts" | "tsx" | "jsx" | "go"
        | "java" | "kt" | "swift" | "rb" | "php" | "pl" | "lua" | "hs" | "ml" | "nix" | "sh" | "fish" | "zsh"
        | "toml" | "yaml" | "yml" | "json" | "xml" | "html" | "css" | "scss" | "vue" | "svelte" | "zig" | "el"
        | "clj" | "ex" | "exs" | "erl" | "scala" | "sql" | "proto" | "cmake" | "mk" => FileKind::Code,
        "o" | "a" | "so" | "rlib" | "rmeta" | "dll" | "exe" | "dylib" | "lib" | "obj" | "class" | "pyc" | "wasm"
        | "bin" | "elf" | "d" | "pdb" | "dwo" | "ko" | "node" | "pch" | "gch" | "dex" | "beam" | "hi" => {
            FileKind::Binary
        }
        "sqlite" | "sqlite3" | "db" | "mdb" | "ldb" | "leveldb" | "realm" | "wal" | "db-wal" | "db-shm" => {
            FileKind::Database
        }
        "ttf" | "otf" | "woff" | "woff2" | "pcf" | "bdf" | "pfb" => FileKind::Font,
        _ => FileKind::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_by_name() {
        assert_eq!(classify(b"Holiday.MP4", false), FileKind::Video);
        assert_eq!(classify(b"sekka-nightly.iso", false), FileKind::DiskImage);
        assert_eq!(classify(b"libfoo.so", false), FileKind::Binary);
        assert_eq!(classify(b"pack-1234.pack", true), FileKind::VersionControl);
        assert_eq!(classify(b"Xorg.0.log.old", false), FileKind::Log);
        assert_eq!(classify(b".bashrc", false), FileKind::Other);
        assert_eq!(classify(b"000301.log", false), FileKind::Database);
        assert_eq!(classify(b"Makefile", false), FileKind::Other);
    }
}
