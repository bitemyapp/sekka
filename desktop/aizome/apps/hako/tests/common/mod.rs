//! What the window tests share: a home folder of their own to browse,
//! and a harness that waits for background work.

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use aizome_kit::testing::Harness;
use aizome_kit::theme::{Appearance, ThemeChoice};
use hako::ui::browser::Load;
use hako::ui::{Hako, Msg, Options};

/// A folder of a test's own, removed when dropped.
pub struct Sandbox(pub PathBuf);

impl Sandbox {
    pub fn new(name: &str) -> Sandbox {
        static N: AtomicUsize = AtomicUsize::new(0);
        let root = option_env!("CARGO_TARGET_TMPDIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
        let dir = root.join(format!("hako-ui-{name}-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Sandbox(std::fs::canonicalize(&dir).unwrap())
    }

    pub fn path(&self) -> &Path {
        &self.0
    }

    pub fn home(&self) -> PathBuf {
        self.0.join("home")
    }

    pub fn file(&self, rel: &str, text: &str) -> PathBuf {
        let p = self.0.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, text).unwrap();
        p
    }

    pub fn dir(&self, rel: &str) -> PathBuf {
        let p = self.0.join(rel);
        std::fs::create_dir_all(&p).unwrap();
        p
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A home folder like a person's: places, a project, pictures, and files
/// of many kinds, with fixed times so pictures of it don't change.
pub fn populate(s: &Sandbox) {
    for d in ["Desktop", "Documents", "Downloads", "Music", "Pictures", "Videos"] {
        s.dir(&format!("home/{d}"));
    }
    s.file("home/Documents/architecture.md", "# Aizome architecture\n\nAizome is Sekka's desktop environment, built on Sidestep's native Rust\ntoolkit.\n\n## Parts and processes\n\n| Part | Crate |\n|---|---|\n| Compositor | aizome-comp |\n");
    s.file("home/Documents/roadmap.md", "# Roadmap\n");
    s.file("home/Documents/budget.csv", "a,b\n1,2\n");
    s.file("home/Downloads/sekka-nightly.iso", &"\0".repeat(4096));
    s.file("home/Downloads/release-notes.pdf", "%PDF-1.7\n");
    s.file("home/Downloads/fonts.tar.gz", "gz");
    s.file("home/work/sidestep/README.md", "# Sidestep\n\nA native Rust UI toolkit for Linux.\n");
    s.file("home/work/sidestep/Cargo.toml", "[workspace]\nmembers = [\"crates/*\"]\n");
    s.file("home/work/sidestep/crates/sidestep-ui/src/lib.rs", "//! A native toolkit.\n");
    s.file("home/work/sidestep/.gitignore", "target\n");
    s.file("home/work/notes.txt", "notes\n");
    s.file("home/.bashrc", "set -o vi\n");
    s.file("home/Music/eighth.flac", "fLaC");
    s.file("home/Videos/tiles.mkv", "\x1aE");
    write_png(&s.0.join("home/Pictures/wallpaper-draft.png"), 64, 40);
    write_png(&s.0.join("home/Pictures/mark.png"), 32, 32);
    // Times fixed: 17 Oct 2026, 21:42 (well, near it, in UTC).
    let t = std::time::UNIX_EPOCH + Duration::from_secs(1_792_273_320);
    set_times(&s.0.join("home"), t);
}

fn set_times(p: &Path, t: std::time::SystemTime) {
    if let Ok(rd) = std::fs::read_dir(p) {
        for e in rd.flatten() {
            set_times(&e.path(), t);
        }
    }
    if let Ok(f) = std::fs::File::open(p) {
        let _ = f.set_times(std::fs::FileTimes::new().set_modified(t).set_accessed(t));
    }
}

/// A small PNG: a diagonal gradient in the board's blues.
pub fn write_png(path: &Path, w: u32, h: u32) {
    let mut data = Vec::new();
    for y in 0..h {
        for x in 0..w {
            let t = (x + y) as f32 / (w + h) as f32;
            data.extend_from_slice(&[(61.0 + 61.0 * t) as u8, (89.0 + 73.0 * t) as u8, (161.0 + 86.0 * t) as u8, 255]);
        }
    }
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    aizome_kit::testing::write_png(path, w, h, &data).unwrap();
}

pub fn appearance(theme: ThemeChoice) -> Appearance {
    Appearance { theme, ..Appearance::default() }
}

/// Hako on `s`'s home, in `theme`.
pub fn harness(s: &Sandbox, theme: ThemeChoice) -> Harness<Hako> {
    let mut opts = Options::sandbox(s.path());
    opts.home = s.home();
    let h = Harness::with(Hako::new(opts), appearance(theme), false);
    let mut h = h;
    settle(&mut h);
    h
}

/// Let background work (listings, thumbnails, jobs) arrive, until every
/// column is read and no job runs (or five seconds pass).
pub fn settle(h: &mut Harness<Hako>) {
    let until = Instant::now() + Duration::from_secs(5);
    loop {
        h.send(Msg::Nop);
        let p = h.program();
        let loading =
            p.windows.values().any(|b| b.columns.iter().any(|c| matches!(c.load, Load::Wanted | Load::Loading(_))));
        let searching = p.windows.values().any(|b| b.search.as_ref().is_some_and(|s| s.running));
        if !loading && p.jobs.is_empty() && !searching {
            // Once more, for what came with the last.
            std::thread::sleep(Duration::from_millis(20));
            h.send(Msg::Nop);
            return;
        }
        if Instant::now() > until {
            return;
        }
        h.advance(Duration::from_millis(20));
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Wait for `cond` (sending nothing but no-ops), up to five seconds.
pub fn wait_for(h: &mut Harness<Hako>, cond: impl Fn(&Hako) -> bool) -> bool {
    let until = Instant::now() + Duration::from_secs(5);
    while Instant::now() < until {
        h.send(Msg::Nop);
        if cond(h.program()) {
            return true;
        }
        h.advance(Duration::from_millis(20));
        std::thread::sleep(Duration::from_millis(5));
    }
    false
}

/// Where test pictures go for looking at.
pub fn out(name: &str) -> PathBuf {
    let dir =
        option_env!("CARGO_TARGET_TMPDIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir).join("hako-shots");
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}
