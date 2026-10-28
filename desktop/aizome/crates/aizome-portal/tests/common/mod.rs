//! What the chooser tests share: a home of their own, requests, and
//! waiting for a folder to be read.
#![allow(dead_code)]

use std::fs::File;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use aizome_kit::testing::Harness;
use aizome_portal::chooser::Chooser;
use aizome_portal::request::{Mode, Request, bytes_of_path};

/// When the fixture's files were last changed: 17 October 2026, 21:42 UTC.
pub const MODIFIED: i64 = 1_792_273_320;

/// A home with folders and files in it, removed afterwards.
pub struct Home(PathBuf);

impl Home {
    pub fn new(name: &str) -> Home {
        let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("chooser-{name}"));
        let _ = std::fs::remove_dir_all(&root);
        for dir in ["Documents/Taxes", "Pictures", "Downloads", ".config"] {
            std::fs::create_dir_all(root.join(dir)).unwrap();
        }
        for (file, text) in [
            ("Documents/notes.md", "# Notes"),
            ("Documents/report.pdf", "%PDF-1.7"),
            ("Documents/.secret", "hidden"),
            ("Pictures/cat.png", "png"),
            ("Pictures/DOG.JPG", "jpg"),
            ("Pictures/readme.txt", "words"),
        ] {
            std::fs::write(root.join(file), text).unwrap();
            File::options()
                .write(true)
                .open(root.join(file))
                .and_then(|f| f.set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(MODIFIED as u64)))
                .unwrap();
        }
        // Folders last: their files changed them.
        for dir in ["Documents/Taxes", "Documents", "Pictures", "Downloads", ".config"] {
            File::open(root.join(dir))
                .and_then(|f| f.set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(MODIFIED as u64)))
                .unwrap();
        }
        Home(root)
    }

    pub fn path(&self, rel: &str) -> PathBuf {
        if rel.is_empty() { self.0.clone() } else { self.0.join(rel) }
    }
}

impl Drop for Home {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub fn request(mode: Mode, folder: &Path) -> Request {
    Request {
        mode,
        parent_window: "wayland:exported-1".into(),
        current_folder: Some(bytes_of_path(folder)),
        ..Request::default()
    }
}

pub fn chooser(home: &Home, request: Request) -> Harness<Chooser> {
    let mut c = Chooser::with_home(request, home.path(".state/file-chooser.json"), home.path(""));
    c.ask_desk = false;
    let mut h = Harness::new(c);
    settle(&mut h);
    h
}

/// Until the location is read.
pub fn settle(h: &mut Harness<Chooser>) {
    assert!(h.settle(Duration::from_secs(10), |c| !c.browser.loading), "the folder wasn't read");
}
