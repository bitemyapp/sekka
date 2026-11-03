//! Pictures for games beyond those beside them: what EmulationStation's
//! and Batocera's `gamelist.xml` say a game's are, and box art fetched
//! from libretro-thumbnails by the name the game's database knows it by,
//! as RetroArch fetches it, then kept (`Dirs::art`).
//!
//! A fetch that finds nothing leaves a `.none` beside where the picture
//! would be, so it isn't asked again for a week; one that can't reach the
//! server leaves nothing, and is tried again next time.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// Where libretro-thumbnails are served.
pub const THUMBNAILS: &str = "https://thumbnails.libretro.com";

/// How long a picture found missing isn't asked for again.
const MISSING_FOR: Duration = Duration::from_secs(7 * 24 * 60 * 60);

/// A game's name as libretro-thumbnails names its files: `&*/:` `` ` ``
/// `<>?\|"` made `_`.
pub fn file_name(name: &str) -> String {
    name.chars().map(|c| if "&*/:`<>?\\|\"".contains(c) { '_' } else { c }).collect()
}

/// Where a game's box art is kept once fetched: by its system and the
/// name its database knows it by.
pub fn cached(art: &Path, system: &str, name: &str) -> PathBuf {
    art.join(system).join(format!("{}.png", file_name(name)))
}

fn marker(path: &Path) -> PathBuf {
    path.with_extension("none")
}

/// Whether a fetch for `path` found nothing within the last week.
pub fn known_missing(path: &Path) -> bool {
    std::fs::metadata(marker(path))
        .and_then(|m| m.modified())
        .is_ok_and(|t| SystemTime::now().duration_since(t).is_ok_and(|age| age < MISSING_FOR))
}

/// A path's part, percent-encoded but for what a URL keeps as it is.
fn encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~!'()*,".contains(&b) {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// The address of box art in a database's folder of libretro-thumbnails
/// (`Nintendo - Super Nintendo Entertainment System`), by its file's name.
pub fn url(base: &str, database: &str, file: &str) -> String {
    format!("{base}/{}/Named_Boxarts/{}.png", encode(database), encode(file))
}

/// How a fetch went.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fetched {
    Found,
    /// Not there: marked, not asked again for a week.
    Missing,
    /// The server couldn't be reached: asked again next time.
    Unreachable,
}

/// An agent for fetching, with timeouts a television can wait out.
pub fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .user_agent(concat!("Chanoma/", env!("CARGO_PKG_VERSION"), " (Aizome)"))
        .timeout_global(Some(Duration::from_secs(20)))
        .timeout_connect(Some(Duration::from_secs(8)))
        .http_status_as_error(false)
        .build()
        .into()
}

/// Fetch the box art `path` is for (as [`cached`] made it: its system's
/// folder, its file's name) from `base`, trying each of its system's
/// databases, and keep it there.
pub fn fetch(agent: &ureq::Agent, base: &str, path: &Path) -> Fetched {
    let system = path.parent().and_then(Path::file_name).map(|n| n.to_string_lossy().into_owned());
    let file = path.file_stem().map(|n| n.to_string_lossy().into_owned());
    let (Some(system), Some(file)) = (system, file) else { return Fetched::Missing };
    for database in crate::metadata::databases(&system) {
        let resp = match agent.get(&url(base, database, &file)).call() {
            Ok(r) => r,
            Err(e) => {
                log::debug!("chanoma: box art for {file}: {e}");
                return Fetched::Unreachable;
            }
        };
        if resp.status() != 200 {
            continue;
        }
        let Ok(bytes) = resp.into_body().read_to_vec() else { return Fetched::Unreachable };
        // A picture, not a page saying there's none.
        if image::guess_format(&bytes).is_err() {
            continue;
        }
        let kept = path.parent().is_some_and(|d| std::fs::create_dir_all(d).is_ok()) && {
            let tmp = path.with_extension("part");
            std::fs::write(&tmp, &bytes).and_then(|()| std::fs::rename(&tmp, path)).is_ok()
        };
        return if kept { Fetched::Found } else { Fetched::Unreachable };
    }
    if let Some(d) = path.parent()
        && std::fs::create_dir_all(d).is_ok()
    {
        let _ = std::fs::write(marker(path), b"");
    }
    Fetched::Missing
}

/// What a system folder's `gamelist.xml` says each game's picture is: its
/// box (`thumbnail`), else its picture (`image`), else its marquee, by
/// the game's path, both beside the list. Games without one aren't in it.
pub fn gamelist(list: &Path) -> HashMap<PathBuf, PathBuf> {
    let Ok(text) = std::fs::read_to_string(list) else { return HashMap::new() };
    let dir = list.parent().unwrap_or(Path::new("."));
    gamelist_in(&text, dir)
}

fn gamelist_in(text: &str, dir: &Path) -> HashMap<PathBuf, PathBuf> {
    let Ok(doc) = roxmltree::Document::parse(text) else { return HashMap::new() };
    let at = |p: &str| {
        let p = p.trim();
        let rel = p.strip_prefix("./").unwrap_or(p);
        if Path::new(rel).is_absolute() { PathBuf::from(rel) } else { dir.join(rel) }
    };
    let mut out = HashMap::new();
    for game in doc.descendants().filter(|n| n.has_tag_name("game")) {
        let child = |tag: &str| {
            game.children().find(|c| c.has_tag_name(tag)).and_then(|c| c.text()).filter(|t| !t.trim().is_empty())
        };
        let Some(path) = child("path") else { continue };
        if let Some(picture) = child("thumbnail").or_else(|| child("image")).or_else(|| child("marquee")) {
            out.insert(at(path), at(picture));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    #[test]
    fn names_and_addresses_are_libretro_thumbnails_own() {
        assert_eq!(file_name("Kirby's Dream Land (USA, Europe)"), "Kirby's Dream Land (USA, Europe)");
        assert_eq!(file_name("Tom & Jerry: Frantic Antics! (USA)"), "Tom _ Jerry_ Frantic Antics! (USA)");
        assert_eq!(
            url(THUMBNAILS, "Nintendo - Super Nintendo Entertainment System", "Super Mario World (USA)"),
            "https://thumbnails.libretro.com/Nintendo%20-%20Super%20Nintendo%20Entertainment%20System/Named_Boxarts/Super%20Mario%20World%20(USA).png"
        );
        assert_eq!(cached(Path::new("/c/art"), "snes", "A/B"), Path::new("/c/art/snes/A_B.png"));
    }

    #[test]
    fn a_gamelist_says_each_games_picture_box_first() {
        let xml = r#"<?xml version="1.0"?>
<gameList>
  <game><path>./Kirby (USA).sfc</path><name>Kirby</name>
    <image>./images/Kirby (USA)-image.png</image><thumbnail>./images/Kirby (USA)-thumb.png</thumbnail></game>
  <game><path>./Zelda &amp; Link.zip</path><image>./images/Zelda-image.png</image></game>
  <game><path>./Bare.sfc</path><name>No pictures</name></game>
  <folder><path>./subdir</path><image>./x.png</image></folder>
</gameList>"#;
        let got = gamelist_in(xml, Path::new("/roms/snes"));
        assert_eq!(got.len(), 2);
        assert_eq!(got[Path::new("/roms/snes/Kirby (USA).sfc")], Path::new("/roms/snes/images/Kirby (USA)-thumb.png"));
        assert_eq!(got[Path::new("/roms/snes/Zelda & Link.zip")], Path::new("/roms/snes/images/Zelda-image.png"));
        assert!(gamelist_in("not xml", Path::new("/")).is_empty());
    }

    /// A server answering each request with one PNG for `found`'s address,
    /// and a 404 for any other, `n` times.
    fn serve(found: &'static str, n: usize) -> String {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            let png: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0\0\0\x01\0\0\0\x01\x08\x06\0\0\0\x1f\x15\xc4\x89";
            for stream in listener.incoming().take(n) {
                let mut s = stream.unwrap();
                let mut buf = [0u8; 4096];
                let got = s.read(&mut buf).unwrap();
                let req = String::from_utf8_lossy(&buf[..got]);
                let path = req.split_whitespace().nth(1).unwrap_or("");
                if path == found {
                    let head = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", png.len());
                    s.write_all(head.as_bytes()).unwrap();
                    s.write_all(png).unwrap();
                } else {
                    s.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
                }
            }
        });
        base
    }

    #[test]
    fn box_art_is_fetched_once_and_a_miss_marked() {
        let art = std::env::temp_dir().join(format!("chanoma-art-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&art);
        // The PC Engine's games are in two databases: the second has it.
        let base = serve("/NEC%20-%20PC%20Engine%20SuperGrafx/Named_Boxarts/Daimakaimura%20(Japan).png", 4);
        let agent = agent();
        let found = cached(&art, "pce", "Daimakaimura (Japan)");
        assert_eq!(fetch(&agent, &base, &found), Fetched::Found);
        assert!(image::guess_format(&std::fs::read(&found).unwrap()).is_ok());
        let missing = cached(&art, "pce", "Nobody's Game (Japan)");
        assert!(!known_missing(&missing));
        assert_eq!(fetch(&agent, &base, &missing), Fetched::Missing);
        assert!(known_missing(&missing) && !missing.exists());
        // Nothing listening: not marked, tried again later.
        let gone = cached(&art, "snes", "Kirby (USA)");
        assert_eq!(fetch(&agent, "http://127.0.0.1:9", &gone), Fetched::Unreachable);
        assert!(!known_missing(&gone));
        let _ = std::fs::remove_dir_all(&art);
    }
}
