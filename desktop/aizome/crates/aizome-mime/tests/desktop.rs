use aizome_mime::{DIRECTORY, EMPTY, MimeDb, TEXT, UNKNOWN, glob_matches};
use std::path::Path;

#[test]
fn shared_database_identifies_names_content_and_relationships() {
    let db = MimeDb::get();
    assert_eq!(db.by_name("cat.PNG").as_deref(), Some("image/png"));
    assert_eq!(db.by_name("backup.tar.gz").as_deref(), Some("application/x-compressed-tar"));
    assert!(db.is_text(&db.by_name("main.rs").expect("shared-mime-info is installed")));
    assert!(db.is_text("application/json"));
    assert!(db.equal("text/x-markdown", "text/markdown"));
    assert!(db.matches("text/plain", "text/x-csrc"));
    assert!(db.matches("IMAGE/*", "image/x-png"));
    assert!(!db.matches("image/*", "text/plain"));
    assert!(!db.is_a("x-scheme-handler/http", UNKNOWN));
    assert!(db.is_a("X-Scheme-Handler/HTTP", "x-scheme-handler/http"));
    assert!(db.is_a(&db.by_content(b"#!/usr/bin/python3\nprint('hello')\n"), "text/x-python"));
    // A container header alone does not tell us whether its tracks are
    // video or audio. In particular it is not evidence for WebM.
    assert_eq!(db.by_content(b"\x1aE\xdf\xa3\x8b\x42\x82\x88matroska").as_ref(), "application/x-matroska");
    assert_eq!(db.by_content(b"hello, world\n").as_ref(), TEXT);
    assert_eq!(db.by_content(b"\0\x01\x02binary").as_ref(), UNKNOWN);
    assert_eq!(db.by_content(b"").as_ref(), EMPTY);
}

#[test]
fn metadata_and_failed_reads_are_not_empty_content() {
    let db = MimeDb::get();
    let tmp = tempfile::tempdir().unwrap();
    let missing = tmp.path().join("missing");
    assert_eq!(db.for_file(&missing, true).as_ref(), UNKNOWN);
    assert_eq!(db.for_file(tmp.path(), true).as_ref(), DIRECTORY);
    let empty = tmp.path().join("empty");
    std::fs::write(&empty, []).unwrap();
    assert_eq!(db.for_file(&empty, true).as_ref(), EMPTY);
    let script = tmp.path().join("script");
    std::fs::write(&script, b"#!/usr/bin/python3\nprint('hello')\n").unwrap();
    assert_eq!(db.for_file(&script, false).as_ref(), UNKNOWN);
    assert!(db.is_a(&db.for_file(&script, true), "text/x-python"));
    assert_eq!(db.guess(Some(Path::new("x.png")), None).mime.as_ref(), "image/png");
}

#[test]
fn chooser_patterns_match_unicode_and_classes() {
    assert!(glob_matches("IMG_????.[jJ][pP][gG]", "img_0042.JPG"));
    assert!(glob_matches("?.txt", "猫.txt"));
    assert!(glob_matches("*.[!c]", "main.h"));
    assert!(glob_matches("*.[^c]", "main.h"));
    assert!(!glob_matches("*.[^c]", "main.c"));
    assert!(glob_matches("[]]x", "]x"));
    assert!(glob_matches("[x", "[x"));
    assert!(glob_matches("[[^]", "^"));
    assert!(glob_matches("*", ".hidden"));
}

#[test]
fn malformed_portal_values_are_rejected() {
    let db = MimeDb::get();
    assert_eq!(db.canonical("text/plain\0extra").as_ref(), UNKNOWN);
    assert!(!db.equal("text/plain\0extra", "text/plain"));
    assert!(!db.matches("text/plain\0extra", "text/plain"));
    assert_eq!(db.by_name("bad\0.png"), None);
    assert_eq!(db.for_file(Path::new("bad\0.png"), true).as_ref(), UNKNOWN);
    use std::os::unix::ffi::OsStringExt;
    let name = std::ffi::OsString::from_vec(b"bad\xff.png".to_vec());
    assert_eq!(db.for_file(Path::new(&name), false).as_ref(), "image/png");
}

#[test]
fn symlinks_and_special_files_do_not_trigger_content_reads() {
    use std::os::unix::{fs::symlink, net::UnixListener};
    let db = MimeDb::get();
    let tmp = tempfile::tempdir().unwrap();
    let socket = tmp.path().join("socket.png");
    let _listener = UnixListener::bind(&socket).unwrap();
    assert_eq!(db.for_file(&socket, true).as_ref(), "inode/socket");
    let dangling = tmp.path().join("dangling.png");
    symlink(tmp.path().join("missing"), &dangling).unwrap();
    assert_eq!(db.for_file(&dangling, true).as_ref(), aizome_mime::SYMLINK);
    symlink(tmp.path(), tmp.path().join("directory-link")).unwrap();
    assert_eq!(db.for_file(&tmp.path().join("directory-link"), true).as_ref(), DIRECTORY);
}
