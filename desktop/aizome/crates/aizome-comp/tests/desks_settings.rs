//! Desks added, renamed or removed in Aizome are written to the settings,
//! so Settei shows them, and a change it saves keeps them (#228).

mod common;

use std::path::PathBuf;
use std::time::{Duration, Instant};

use aizome_comp_core::aizome_config::{DeskConfig, Settings};
use aizome_ipc::{DeskSel, Request, Response};
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

fn eventually(c: &Compositor, what: &str, done: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !done() {
        assert!(Instant::now() < deadline, "{what}:\n{}", c.log_text());
        std::thread::sleep(Duration::from_millis(50));
    }
}

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
