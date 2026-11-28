//! File events must work without waiting for the polling fallback.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use aizome_config::Watcher;

struct Temp(PathBuf);

impl Temp {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "aizome-settings-watch-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn watch(path: &std::path::Path) -> (Watcher, Receiver<Option<String>>) {
    let (tx, rx) = mpsc::channel();
    let read = path.to_owned();
    let watcher = Watcher::spawn_for(path.to_owned(), Duration::from_secs(3600), move || {
        let _ = tx.send(std::fs::read_to_string(&read).ok());
    });
    (watcher, rx)
}

fn saw(rx: &Receiver<Option<String>>, expected: Option<&str>) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let got = rx.recv_timeout(deadline.saturating_duration_since(Instant::now())).expect("file change");
        if got.as_deref() == expected {
            return;
        }
    }
}

#[test]
fn writes_atomic_saves_deletion_and_recreation_are_observed() {
    let dir = Temp::new();
    let path = dir.0.join("settings.toml");
    std::fs::write(&path, "old").unwrap();
    let (_watcher, rx) = watch(&path);

    std::fs::write(&path, "new").unwrap();
    saw(&rx, Some("new"));
    let tmp = dir.0.join("settings.tmp");
    std::fs::write(&tmp, "replacement").unwrap();
    std::fs::rename(&tmp, &path).unwrap();
    saw(&rx, Some("replacement"));
    std::fs::write(&path, "edited after rename").unwrap();
    saw(&rx, Some("edited after rename"));
    std::fs::remove_file(&path).unwrap();
    saw(&rx, None);
    std::fs::write(&path, "created again").unwrap();
    saw(&rx, Some("created again"));
}

#[test]
fn absent_parents_and_replaced_directories_are_followed() {
    let dir = Temp::new();
    let parent = dir.0.join("config/aizome");
    let path = parent.join("settings.toml");
    let (_watcher, rx) = watch(&path);
    std::fs::create_dir_all(&parent).unwrap();
    std::fs::write(&path, "first").unwrap();
    saw(&rx, Some("first"));

    std::fs::rename(&parent, dir.0.join("previous")).unwrap();
    saw(&rx, None);
    std::fs::create_dir(&parent).unwrap();
    std::fs::write(&path, "second").unwrap();
    saw(&rx, Some("second"));
    std::fs::remove_dir_all(&parent).unwrap();
    saw(&rx, None);
    std::fs::create_dir(&parent).unwrap();
    std::fs::write(&path, "third").unwrap();
    saw(&rx, Some("third"));

    std::fs::rename(dir.0.join("config"), dir.0.join("old-config")).unwrap();
    saw(&rx, None);
    std::fs::create_dir_all(&parent).unwrap();
    std::fs::write(&path, "new ancestor").unwrap();
    saw(&rx, Some("new ancestor"));
}

#[test]
fn writes_through_a_settings_symlink_are_observed() {
    let dir = Temp::new();
    let target_dir = Temp::new();
    let target = target_dir.0.join("real.toml");
    std::fs::write(&target, "first").unwrap();
    let path = dir.0.join("settings.toml");
    std::os::unix::fs::symlink(&target, &path).unwrap();
    let (_watcher, rx) = watch(&path);
    std::fs::write(&target, "second").unwrap();
    saw(&rx, Some("second"));
    std::fs::remove_file(&target).unwrap();
    saw(&rx, None);
    std::fs::write(&target, "recreated target").unwrap();
    saw(&rx, Some("recreated target"));
    let tmp = target_dir.0.join("replacement");
    std::fs::write(&tmp, "atomic target").unwrap();
    std::fs::rename(&tmp, &target).unwrap();
    saw(&rx, Some("atomic target"));
}

#[test]
fn a_retargeted_directory_symlink_is_followed() {
    let dir = Temp::new();
    let first = Temp::new();
    let second = Temp::new();
    std::fs::write(first.0.join("settings.toml"), "first").unwrap();
    std::fs::write(second.0.join("settings.toml"), "second").unwrap();
    let link = dir.0.join("aizome");
    std::os::unix::fs::symlink(&first.0, &link).unwrap();
    let (_watcher, rx) = watch(&link.join("settings.toml"));
    let replacement = dir.0.join("replacement");
    std::os::unix::fs::symlink(&second.0, &replacement).unwrap();
    std::fs::rename(&replacement, &link).unwrap();
    saw(&rx, Some("second"));
    std::fs::write(second.0.join("settings.toml"), "updated").unwrap();
    saw(&rx, Some("updated"));
}

#[test]
fn unrelated_writes_and_reads_do_not_notify_and_drop_stops_an_idle_watch() {
    let dir = Temp::new();
    let path = dir.0.join("settings.toml");
    std::fs::write(&path, "unchanged").unwrap();
    let (watcher, rx) = watch(&path);
    std::fs::write(dir.0.join("other.toml"), "irrelevant").unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "unchanged");
    assert!(matches!(rx.recv_timeout(Duration::from_millis(100)), Err(mpsc::RecvTimeoutError::Timeout)));

    let (done, stopped) = mpsc::channel();
    std::thread::spawn(move || {
        drop(watcher);
        done.send(()).unwrap();
    });
    stopped.recv_timeout(Duration::from_secs(5)).expect("drop interrupts the blocking wait");
    std::fs::write(&path, "after drop").unwrap();
    assert!(matches!(rx.recv_timeout(Duration::from_millis(100)), Err(mpsc::RecvTimeoutError::Disconnected)));
}
