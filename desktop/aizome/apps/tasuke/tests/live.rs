//! The window on the real agent: a message starts a turn on the agent's
//! thread, the turn's waker wakes the window, and its events reach the
//! transcript; the conversation is saved where the agent keeps them.
//!
//! Everything is under a scratch directory: the settings name a catalog
//! model, and its files are there as sparse files of the catalog's sizes
//! (what the store checks), so the window takes it as downloaded. No model
//! server runs: the turn ends with the agent saying it can't be reached,
//! and the window's look at the server says it's stopped.

#![cfg(feature = "live")]

use std::time::Duration;

use aizome_kit::testing::Harness;
use aizome_kit::theme::Appearance;
use tasuke_models::{Catalog, ModelId};
use tasuke_window::app::{Msg, Page, Tasuke};
use tasuke_window::transcript::Body;

#[test]
fn a_turn_runs_on_the_agent_and_its_events_reach_the_window() {
    let root = std::env::temp_dir().join(format!("tasuke-window-live-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let (config, data) = (root.join("config"), root.join("data"));
    // SAFETY: this test binary has this one test, so nothing else reads
    // the environment while it's set.
    unsafe {
        std::env::set_var("XDG_CONFIG_HOME", &config);
        std::env::set_var("XDG_DATA_HOME", &data);
        std::env::set_var("TASUKE_DATA_DIR", data.join("tasuke"));
        std::env::remove_var("XDG_RUNTIME_DIR");
    }
    std::fs::create_dir_all(config.join("tasuke")).unwrap();
    // A port nothing listens on, and not Tasuke's own: the agent then
    // doesn't try to start the user's model server unit.
    std::fs::write(config.join("tasuke/config.toml"), "[model]\nid = \"gemma-4-12b\"\n[server]\nport = 9\n").unwrap();
    let catalog = Catalog::load_default();
    let entry = catalog.get(&ModelId::new("gemma-4-12b")).expect("in the catalog");
    let dir = data.join("tasuke/models/gemma-4-12b");
    std::fs::create_dir_all(&dir).unwrap();
    for f in &entry.files {
        std::fs::File::create(dir.join(f.file_name())).unwrap().set_len(f.size.0).unwrap();
    }

    let mut h = Harness::with(Tasuke::live(), Appearance::default(), false);
    assert!(h.program().first_run.is_none(), "a model is chosen and downloaded");
    assert_eq!(h.program().chip.name, "Gemma 4 12B");
    let Page::Conversation(id) = h.program().page else { panic!("a conversation first") };
    h.send(Msg::Suggest(id, "Is my sound on?".into()));
    assert!(h.program().conversation(id).unwrap().turn.is_some(), "the turn started");
    let ended = h.settle(Duration::from_secs(30), |t| t.conversation(id).is_some_and(|c| c.turn.is_none()));
    assert!(ended, "the turn ended and the window heard it");
    let c = h.program().conversation(id).unwrap();
    assert!(!c.transcript.busy);
    assert!(matches!(c.transcript.items.first().map(|i| &i.body), Some(Body::User { .. })));
    assert!(
        c.transcript.items.iter().skip(1).any(|i| matches!(i.body, Body::Error { .. } | Body::Answer(_))),
        "the agent's answer or error is shown: {:?}",
        c.transcript.items
    );
    // The window looked at the server (on the pool) and says what it found.
    h.settle(Duration::from_secs(10), |t| t.models.server != "Looking…");
    assert!(
        h.program().models.server.starts_with("Stopped") || h.program().models.server.starts_with("Not set up"),
        "{}",
        h.program().models.server
    );
    // Saved conversations are read after the window opens.
    assert!(!h.program().loading);
    let saved = std::fs::read_dir(data.join("tasuke/sessions")).unwrap().count();
    assert!(saved >= 1, "the conversation is saved");
    let _ = std::fs::remove_dir_all(&root);
}
