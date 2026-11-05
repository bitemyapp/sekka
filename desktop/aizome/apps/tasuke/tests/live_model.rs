//! The window on a real model: a question goes through the real agent to
//! the model server the settings name, and the answer comes back to the
//! transcript. Unlike `live.rs`, nothing stands in, so it's ignored unless
//! asked for. Run it with Tasuke's model server running (`tasuke server
//! start`) and `XDG_CONFIG_HOME` and `XDG_DATA_HOME` naming its settings
//! and downloaded model:
//!
//! ```text
//! cargo test -p aizome-tasuke --test live_model -- --ignored --nocapture
//! ```
//!
//! With `TASUKE_LIVE_SHOT` set, the answered conversation is drawn there.
//! The question only needs looking, and nothing here presses Run, so it
//! changes nothing on the computer it runs on.

#![cfg(feature = "live")]

use std::path::Path;
use std::time::{Duration, Instant};

use aizome_kit::testing::{Harness, write_png};
use aizome_kit::theme::Appearance;
use tasuke_window::app::{Msg, Page, Tasuke};
use tasuke_window::transcript::Body;

const QUESTION: &str = "How much disk space is free here, and what is using the most memory right now?";

#[test]
#[ignore = "needs Tasuke's model server running with a downloaded model"]
fn a_real_model_answers_after_looking() {
    let mut h = Harness::with(Tasuke::live(), Appearance::default(), false);
    assert!(h.program().first_run.is_none(), "the settings name a downloaded model");
    let Page::Conversation(id) = h.program().page else { panic!("a conversation first") };
    let asked = Instant::now();
    h.send(Msg::Suggest(id, QUESTION.into()));
    // A cold start loads the model first.
    let ended = h.settle(Duration::from_secs(600), |t| t.conversation(id).is_some_and(|c| c.turn.is_none()));
    let took = asked.elapsed();
    let items = &h.program().conversation(id).unwrap().transcript.items;
    assert!(ended, "the turn ended within ten minutes: {items:?}");
    assert!(!items.iter().any(|i| matches!(i.body, Body::Error { .. })), "no error: {items:?}");
    let looks = items.iter().filter(|i| matches!(&i.body, Body::Tool(t) if t.looks)).count();
    assert!(looks >= 1, "it looked before answering: {items:?}");
    let answer = items.iter().rev().find_map(|i| match &i.body {
        Body::Answer(a) => Some(a.text.clone()),
        _ => None,
    });
    let answer = answer.expect("an answer");
    eprintln!("{} looked {looks} times and answered in {:.1} s:\n{answer}", h.program().chip.name, took.as_secs_f64());
    if let Some(path) = std::env::var_os("TASUKE_LIVE_SHOT") {
        let w = h.window();
        let (width, height, px) = h.render_rgba(w, 1.0);
        write_png(Path::new(&path), width, height, &px).expect("the picture written");
    }
}
