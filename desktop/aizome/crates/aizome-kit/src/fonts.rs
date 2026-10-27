//! Aizome's fonts: Archivo for the interface, IBM Plex Mono for code,
//! shortcuts and numbers, Noto Sans Symbols 2 for ❖ ⇧ ⌥ ⌃. The kit
//! installs, for this process, the faces in `$AIZOME_FONT_DIR`, which then
//! win over the system's copies of their families (`nix build .#aizome-fonts`
//! makes that folder: the files the session installs), and the families
//! fontconfig lacks from `../share/aizome/fonts` beside the executable;
//! where neither has one, the system's sans and mono stand in.
//! Nothing is installed into the user's home or the system.

use std::path::{Path, PathBuf};
use std::sync::Once;

use aizome_theme::typeface;
use sidestep_ui::Font;

/// The families, and the file names (stems) that hold them, in the order
/// they're preferred: one file per face, TrueType before OpenType.
const FAMILIES: &[(&str, &[&str])] = &[
    (typeface::UI, &["Archivo[wdth,wght]", "Archivo-VF", "Archivo-Italic[wdth,wght]", "Archivo-Italic-VF"]),
    (
        typeface::MONO,
        &[
            "IBMPlexMono-Regular",
            "IBMPlexMono-Medium",
            "IBMPlexMono-SemiBold",
            "IBMPlexMono-Bold",
            "IBMPlexMono-Italic",
        ],
    ),
    (typeface::SYMBOLS, &["NotoSansSymbols2-Regular"]),
];

static INSTALL: Once = Once::new();

/// Install what's missing, once per process. `$AIZOME_FONT_DIR`'s faces
/// win over the system's copies of a family: pictures (goldens) then come
/// out the same wherever they're drawn, whatever versions the system has
/// (a font installed for the process is found before the system's of the
/// same name). Without it, only the families fontconfig lacks are
/// installed, from beside the executable.
pub fn install() {
    INSTALL.call_once(|| {
        let explicit = std::env::var_os("AIZOME_FONT_DIR").map(PathBuf::from).filter(|d| d.is_dir());
        let dirs = font_dirs();
        for (family, stems) in FAMILIES {
            if let Some(dir) = &explicit
                && install_from(dir, stems)
            {
                continue;
            }
            if has_family(family) {
                continue;
            }
            if !dirs.iter().any(|dir| install_from(dir, stems)) {
                log::info!("{family} isn't installed; the system's font stands in");
            }
        }
    });
}

/// Install the faces `stems` names from `dir`: whether any was.
fn install_from(dir: &Path, stems: &[&str]) -> bool {
    let mut seen_faces: Vec<String> = Vec::new();
    for stem in stems {
        // A variable font's roman and italic files differ, but "-VF" and
        // "[wdth,wght]" copies of one file don't.
        let face = stem.replace("[wdth,wght]", "").replace("-VF", "");
        if seen_faces.contains(&face) {
            continue;
        }
        for ext in ["ttf", "otf"] {
            let path = dir.join(format!("{stem}.{ext}"));
            if path.is_file() {
                match Font::install_file(&path) {
                    Ok(f) if !f.is_empty() => {
                        seen_faces.push(face.clone());
                        log::debug!("installed {} from {}", f.join(", "), path.display());
                    }
                    Ok(_) => log::warn!("{} isn't a font", path.display()),
                    Err(e) => log::warn!("{}: {e}", path.display()),
                }
                break;
            }
        }
    }
    !seen_faces.is_empty()
}

fn has_family(name: &str) -> bool {
    Font::named(name, 13.0).is_some_and(|f| f.family_name().eq_ignore_ascii_case(name))
}

/// Where fonts are looked for.
pub fn font_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(d) = std::env::var_os("AIZOME_FONT_DIR") {
        dirs.push(PathBuf::from(d));
    }
    if let Ok(exe) = std::env::current_exe()
        && let Some(bin) = exe.parent()
    {
        dirs.push(bin.join("../share/aizome/fonts"));
        // Running from a cargo target directory in the repository.
        dirs.push(bin.join("../../share/aizome/fonts"));
    }
    dirs.retain(|d: &PathBuf| Path::new(d).is_dir());
    dirs
}
