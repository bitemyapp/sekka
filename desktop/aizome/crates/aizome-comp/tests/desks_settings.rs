//! Desks added, renamed or removed in Aizome are written to the settings,
//! so Settei shows them, and a change it saves keeps them (#228): where
//! the settings are the compositor's to write, without leaving it ahead of
//! a file it couldn't write, and said at once so that nobody saves over
//! them from an older copy.

mod common;

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use aizome_comp_core::aizome_config::{DeskConfig, Settings};
use aizome_ipc::{Client, DeskSel, Event, Request, Response};
use common::desktop::{program, shell_msg, shell_state, wait};
use common::*;

fn names(desks: &[impl AsRef<str>]) -> Vec<String> {
    desks.iter().map(|d| d.as_ref().to_owned()).collect()
}

fn live(c: &Compositor) -> Vec<String> {
    match c.request(Request::Desks) {
        Response::Desks { desks, .. } => desks.into_iter().map(|d| d.name).collect(),
        other => panic!("{other:?}"),
    }
}

fn path(c: &Compositor) -> PathBuf {
    c.dir.join("config/aizome/settings.toml")
}

fn saved(c: &Compositor) -> Settings {
    Settings::load_from(&path(c)).expect("the settings")
}

fn saved_names(c: &Compositor) -> Vec<String> {
    saved(c).desks.into_iter().map(|d| d.name).collect()
}

fn add(c: &Compositor, name: &str) {
    c.ok(Request::AddDesk { name: name.into(), folder: None, index: None });
}

fn eventually(c: &Compositor, what: &str, done: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !done() {
        assert!(Instant::now() < deadline, "{what}:\n{}", c.log_text());
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Another program's change to the file, here once the compositor has
/// applied it (it looks at the file once a second).
fn applied(c: &Compositor, change: impl FnOnce()) {
    let count = || c.log_text().matches("settings applied").count();
    let before = count();
    change();
    eventually(c, "the settings applied", || count() > before);
}

/// A directory nothing can be written in, until this goes.
struct ReadOnly(PathBuf);

impl ReadOnly {
    fn new(dir: &Path) -> ReadOnly {
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o555)).unwrap();
        ReadOnly(dir.to_owned())
    }
}

impl Drop for ReadOnly {
    fn drop(&mut self) {
        let _ = std::fs::set_permissions(&self.0, std::fs::Permissions::from_mode(0o755));
    }
}

const LEAN: &str = "[appearance]\neffects = \"lean\"\n";

#[test]
fn desks_added_here_are_saved_and_a_settings_change_keeps_them() {
    let c = Compositor::start(Options::default());
    // Four by default, written nowhere.
    assert!(saved(&c).desks.is_empty());
    for name in ["5", "6", "7"] {
        c.ok(Request::AddDesk { name: name.into(), folder: None, index: None });
    }
    let seven = names(&["1", "2", "3", "4", "5", "6", "7"]);
    assert_eq!(live(&c), seven);
    let file = saved(&c);
    assert_eq!(file.desks.iter().map(|d| d.name.clone()).collect::<Vec<_>>(), seven);
    // The rest of the file as it was.
    assert_eq!(file.appearance.effects, Settings::from_toml(Options::default().settings).unwrap().appearance.effects);

    // Settei adds an eighth to the seven it now sees: none are lost.
    let mut s = saved(&c);
    s.desks.push(DeskConfig::named("8"));
    s.save_to(&path(&c)).unwrap();
    eventually(&c, "the eighth Desk", || live(&c).len() == 8);
    assert_eq!(live(&c), names(&["1", "2", "3", "4", "5", "6", "7", "8"]));

    // Renamed and removed here, the file follows.
    c.ok(Request::RenameDesk { desk: DeskSel::Index(2), name: "Mail".into() });
    c.ok(Request::RemoveDesk { desk: DeskSel::Index(8) });
    let file = saved(&c);
    assert_eq!(
        file.desks.iter().map(|d| d.name.clone()).collect::<Vec<_>>(),
        names(&["1", "Mail", "3", "4", "5", "6", "7"])
    );
    // And back to the four defaults: no Desks in the file.
    for i in (5..=7).rev() {
        c.ok(Request::RemoveDesk { desk: DeskSel::Index(i) });
    }
    c.ok(Request::RenameDesk { desk: DeskSel::Index(2), name: "2".into() });
    assert!(saved(&c).desks.is_empty());
    assert_eq!(live(&c), names(&["1", "2", "3", "4"]));
}

/// Desks added while the file doesn't parse can't be saved. They stay when
/// the file is fixed, and the next change here saves them, with what the
/// fix changed.
#[test]
fn desks_added_while_the_file_does_not_parse_stay_when_it_is_fixed() {
    let c = Compositor::start(Options::default());
    let broken = "[appearance\neffects = \"austere\"\n";
    std::fs::write(path(&c), broken).unwrap();
    add(&c, "5");
    add(&c, "6");
    let six = names(&["1", "2", "3", "4", "5", "6"]);
    assert_eq!(live(&c), six);
    assert_eq!(std::fs::read_to_string(path(&c)).unwrap(), broken, "a file someone is fixing is left alone");

    applied(&c, || std::fs::write(path(&c), LEAN).unwrap());
    assert_eq!(live(&c), six, "the fixed file names no Desks, as before: these stay");

    add(&c, "7");
    assert_eq!(saved_names(&c), names(&["1", "2", "3", "4", "5", "6", "7"]));
    assert_eq!(saved(&c).appearance, Settings::from_toml(LEAN).unwrap().appearance);
}

/// A Desk added while the settings can't be written (their directory is
/// read-only, the disk is full) stays when another program then changes
/// something else, and the next change here saves it.
#[test]
fn a_desk_added_while_the_settings_cannot_be_written_stays() {
    let c = Compositor::start(Options::default());
    let dir = path(&c).parent().unwrap().to_owned();
    let read_only = ReadOnly::new(&dir);
    if std::fs::write(dir.join("probe"), "").is_ok() {
        eprintln!("this user writes in a read-only directory (root?): nothing to check");
        return;
    }
    add(&c, "5");
    let five = names(&["1", "2", "3", "4", "5"]);
    assert_eq!(live(&c), five);
    assert!(saved(&c).desks.is_empty(), "nothing could be written");
    assert!(c.log_text().contains("saving the Desks"), "and the log says so:\n{}", c.log_text());
    drop(read_only);

    applied(&c, || {
        let mut s = saved(&c);
        s.appearance = Settings::from_toml(LEAN).unwrap().appearance;
        s.save_to(&path(&c)).unwrap();
    });
    assert_eq!(live(&c), five, "another setting changed: the Desks here stay");

    add(&c, "6");
    assert_eq!(saved_names(&c), names(&["1", "2", "3", "4", "5", "6"]));
}

/// A headless or nested compositor started inside someone's session reads
/// their own settings (`~/.config`, as `scripts/headless-session` runs
/// it), which their login session follows: Desks changed in it stay in it.
/// Settings given to it alone are its own to write: a configuration
/// directory of its own (every other test here), or a file named with
/// `--config`, even that one.
#[test]
fn desks_changed_beside_the_login_session_leave_the_persons_settings_alone() {
    let c = Compositor::start(Options { persons_settings: true, ..Default::default() });
    let persons = c.dir.join("home/.config/aizome/settings.toml");
    let before = std::fs::read_to_string(&persons).unwrap();
    add(&c, "5");
    c.ok(Request::RenameDesk { desk: DeskSel::Index(2), name: "Mail".into() });
    c.ok(Request::RemoveDesk { desk: DeskSel::Index(4) });
    assert_eq!(live(&c), names(&["1", "Mail", "3", "5"]));
    assert_eq!(std::fs::read_to_string(&persons).unwrap(), before, "the person's settings as they were");
    assert!(!c.dir.join("config/aizome/settings.toml").exists());

    let file = persons.to_string_lossy().into_owned();
    let given = Compositor::start(Options { args: &["--config", &file], ..Default::default() });
    add(&given, "5");
    let written: Vec<String> = Settings::load_from(&persons).unwrap().desks.into_iter().map(|d| d.name).collect();
    assert_eq!(written, names(&["1", "2", "3", "4", "5"]));
}

/// The compositor says `config-changed` as it writes the Desks, so that a
/// program holding the settings reads them again before it saves anything
/// (not a second later, when the compositor next looks at the file).
#[test]
fn desks_written_to_the_settings_are_announced_at_once() {
    let c = Compositor::start(Options::default());
    let mut events = Client::connect_to(&c.ipc).unwrap().events().unwrap();
    // On the subscriber's own connection the response comes after the
    // events sent before it: these are what the request itself said.
    let request = Request::AddDesk { name: "5".into(), folder: None, index: None };
    assert!(matches!(events.request(&request).unwrap(), Response::Ok));
    events.stream().set_read_timeout(Some(Duration::from_millis(1))).unwrap();
    let said: Vec<Event> = events.by_ref().map_while(Result::ok).collect();
    assert!(said.iter().any(|e| matches!(e, Event::ConfigChanged)), "{said:?}");
    assert_eq!(saved_names(&c), names(&["1", "2", "3", "4", "5"]));
}

/// The real shell folds its Deskbar the moment after a Desk is added
/// here, which saves its settings: the Desk stays, here and in the file.
/// (The shell saved the settings whole from the copy it held, read before
/// the Desk was added, and the compositor then matched the file.)
#[test]
#[ignore = "requires AIZOME_LAYOUT_TEST_SHELL pointing at the shell under test"]
fn the_real_shells_save_keeps_a_desk_added_a_moment_before() {
    let shell = std::env::var("AIZOME_LAYOUT_TEST_SHELL").expect("shell binary");
    let c = Compositor::start(Options::default());
    let _shell = program(&c, &shell, &[], "shell");
    wait("shell ready", || {
        shell_state(&c, &shell).is_some_and(|s| s["screens"].as_array().is_some_and(|a| !a.is_empty()))
    });
    add(&c, "5");
    shell_msg(&c, &shell, serde_json::json!({"request":"deskbar", "collapsed":true}));
    wait("the shell's save", || saved(&c).deskbar_settings().collapsed);
    // Past the compositor's and the shell's next look at the file.
    std::thread::sleep(Duration::from_millis(2500));
    let five = names(&["1", "2", "3", "4", "5"]);
    assert_eq!(live(&c), five);
    assert_eq!(saved_names(&c), five);
    assert!(saved(&c).deskbar_settings().collapsed);
}
