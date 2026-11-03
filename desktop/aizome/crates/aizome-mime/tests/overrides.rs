//! Its own test process: GIO's MIME directories are process-wide.
//! The fixture is compiled by shared-mime-info, including its binary cache.
use aizome_mime::MimeDb;
use std::path::Path;

fn database(dir: &Path, types: &str) {
    std::fs::create_dir_all(dir.join("packages")).unwrap();
    std::fs::write(dir.join("packages/audit.xml"), format!(
        "<?xml version=\"1.0\"?><mime-info xmlns=\"http://www.freedesktop.org/standards/shared-mime-info\">{types}</mime-info>"
    )).unwrap();
    let output = std::process::Command::new("update-mime-database")
        .arg(dir)
        .output()
        .expect("install shared-mime-info (nix develop .#aizome)");
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
}

static MIME_DIRS: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn fixture() -> tempfile::TempDir {
    let tmp = tempfile::tempdir().unwrap();
    let system = tmp.path().join("system");
    let user = tmp.path().join("user");
    database(
        &system,
        r#"
      <mime-type type="application/x-aizome-long"><glob pattern="*.audit.gz" weight="20"/></mime-type>
      <mime-type type="application/x-aizome-short"><glob pattern="*.gz" weight="80"/></mime-type>
      <mime-type type="application/x-aizome-light"><glob pattern="*.weighted" weight="20"/></mime-type>
      <mime-type type="application/x-aizome-heavy"><glob pattern="*.weighted" weight="80"/></mime-type>
      <mime-type type="text/x-aizome-case"><glob pattern="casecheck" case-sensitive="true"/></mime-type>
      <mime-type type="application/x-aizome-delete"><glob pattern="*.obsolete"/></mime-type>
      <mime-type type="application/x-aizome-parent"><sub-class-of type="text/plain"/></mime-type>
      <mime-type type="application/x-aizome-child">
        <alias type="application/x-aizome-alias"/>
        <sub-class-of type="application/x-aizome-parent"/>
        <comment>Aizome fixture document</comment>
      </mime-type>
      <mime-type type="application/x-aizome-first">
        <glob pattern="*.ambiguous"/>
        <magic priority="80"><match type="string" value="FIRST" offset="0"/></magic>
      </mime-type>
      <mime-type type="application/x-aizome-second">
        <glob pattern="*.ambiguous"/>
        <magic priority="80"><match type="string" value="SECOND" offset="0"/></magic>
      </mime-type>
    "#,
    );
    database(
        &user,
        r#"
      <mime-type type="application/x-aizome-delete"><glob-deleteall/><glob pattern="*.retained"/></mime-type>
    "#,
    );
    gio::content_type_set_mime_dirs(&[user.to_str().unwrap(), system.to_str().unwrap()]);
    tmp
}

#[test]
fn database_overrides_weights_case_aliases_and_ambiguous_names() {
    let _guard = MIME_DIRS.lock().unwrap();
    let tmp = fixture();
    let db = MimeDb::get();
    // GIO prefers a compound extension over its shorter suffix. Weight
    // disambiguates competing matches for the same suffix.
    assert_eq!(db.by_name("test.audit.gz").as_deref(), Some("application/x-aizome-long"));
    assert_eq!(db.by_name("test.weighted").as_deref(), Some("application/x-aizome-heavy"));
    assert_eq!(db.by_name("casecheck").as_deref(), Some("text/x-aizome-case"));
    assert_eq!(db.by_name("CASECHECK"), None);
    assert_eq!(db.by_name("changes.retained").as_deref(), Some("application/x-aizome-delete"));
    assert_eq!(db.canonical("application/x-aizome-alias").as_ref(), "application/x-aizome-child");
    assert!(db.is_text("application/x-aizome-alias"));
    assert_eq!(db.describe("application/x-aizome-alias"), "Aizome fixture document");
    assert!(db.guess(Some(Path::new("file.ambiguous")), None).uncertain);
    let file = tmp.path().join("file.ambiguous");
    std::fs::write(&file, b"SECOND content").unwrap();
    assert_eq!(db.for_file(&file, true).as_ref(), "application/x-aizome-second");
}

/// Tracks a limitation inherited from GIO, not a private parser to maintain.
/// Run explicitly when updating GLib; remove the ignore when upstream fixes it.
#[test]
#[ignore = "GIO does not honor glob-deleteall across MIME database directories; see DEVELOPERS.md"]
fn glob_deleteall_removes_system_patterns() {
    let _guard = MIME_DIRS.lock().unwrap();
    let _tmp = fixture();
    assert_eq!(MimeDb::get().by_name("changes.obsolete"), None);
}
