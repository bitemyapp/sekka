//! The window driven as a person would: a suggestion sent and played
//! from its script, a card answered, a turn stopped, pages switched with
//! each conversation's draft kept.

use std::time::Duration;

use aizome_kit::testing::Harness;
use aizome_kit::theme::Appearance;
use tasuke_window::agent::Decision;
use tasuke_window::app::{Msg, Page, Tasuke};
use tasuke_window::transcript::{Body, ToolState};

fn harness(fixture: &str) -> Harness<Tasuke> {
    Harness::with(Tasuke::fixture(fixture).expect("a fixture"), Appearance::default(), false)
}

fn current(h: &Harness<Tasuke>) -> tasuke_window::app::ConvId {
    match h.program().page {
        Page::Conversation(id) => id,
        other => panic!("on {other:?}"),
    }
}

#[test]
fn a_suggestion_plays_its_turn_and_waits_on_the_card() {
    let mut h = harness("home");
    let w = h.window();
    // The sidebar has a conversation of that name too: press the suggestion.
    let id = current(&h);
    h.send(Msg::Suggest(id, "My sound stopped working".into()));
    assert_eq!(current(&h), id);
    // Streaming: the turn runs, then stops at the card.
    h.advance(Duration::from_secs(20));
    let c = h.program().conversation(id).unwrap();
    assert_eq!(c.title, "My sound stopped working");
    let pending = c.transcript.pending_approval().map(|t| t.call_id.clone()).expect("the card waits");
    assert!(h.texts(w).iter().any(|t| t == "Run this command?"), "{:?}", h.texts(w));
    // Return in the composer sends nothing while a turn runs, and no key
    // answers the card.
    h.type_text(w, "\n");
    assert!(h.program().conversation(id).unwrap().transcript.pending_approval().is_some());
    assert!(h.click_text(w, "Run"));
    h.advance(Duration::from_secs(20));
    let c = h.program().conversation(id).unwrap();
    assert!(c.turn.is_none() && !c.transcript.busy, "the turn ended");
    assert_eq!(c.transcript.tool(&pending).unwrap().state, ToolState::Done { exit: Some(0) });
    assert!(h.texts(w).iter().any(|t| t.starts_with("You chose Run")), "the decision stays: {:?}", h.texts(w));
    let Some(Body::Answer(a)) = c.transcript.items.last().map(|i| &i.body) else { panic!() };
    assert!(a.text.starts_with("Sound is back"));
}

#[test]
fn dont_run_is_kept_and_nothing_runs() {
    let mut h = harness("diagnose");
    let w = h.window();
    let id = current(&h);
    assert!(h.click_text(w, "Don't run"));
    h.advance(Duration::from_secs(10));
    let c = h.program().conversation(id).unwrap();
    assert!(!c.transcript.busy);
    assert!(h.texts(w).iter().any(|t| t.starts_with("You chose Don't run")), "{:?}", h.texts(w));
    let Some(Body::Answer(a)) = c.transcript.items.last().map(|i| &i.body) else { panic!() };
    assert!(a.text.starts_with("All right, nothing was deleted"));
}

#[test]
fn stop_ends_a_turn_and_withdraws_its_card() {
    let mut h = harness("root-approval");
    let w = h.window();
    let id = current(&h);
    assert!(h.click_text(w, "Stop"));
    let c = h.program().conversation(id).unwrap();
    assert!(c.turn.is_none() && c.transcript.pending_approval().is_none());
    assert!(h.texts(w).iter().any(|t| t.starts_with("The turn ended before you answered")), "{:?}", h.texts(w));
}

#[test]
fn drafts_and_pages_are_kept_when_switching() {
    let mut h = harness("diagnose");
    let w = h.window();
    let first = current(&h);
    let field = h.find_id(w, "composer").expect("the composer");
    h.click(w, field.center());
    h.type_text(w, "and the Downloads folder?");
    h.send(Msg::Show(Page::Models));
    assert!(h.texts(w).iter().any(|t| t == "This computer" || t == "THIS COMPUTER"), "{:?}", h.texts(w));
    h.send(Msg::Show(Page::Conversation(first)));
    assert_eq!(h.program().conversation(first).unwrap().draft.text(), "and the Downloads folder?");
}

#[test]
fn a_download_runs_to_the_end_and_is_used() {
    let mut h = harness("first-run");
    let w = h.window();
    let download = h.texts(w).into_iter().find(|t| t.starts_with("Download ")).expect("the download button");
    assert!(h.click_text(w, &download));
    h.advance(Duration::from_secs(2));
    assert!(h.texts(w).iter().any(|t| t.contains("left")), "{:?}", h.texts(w));
    h.advance(Duration::from_secs(1200));
    assert!(h.texts(w).iter().any(|t| t == "Ready."), "{:?}", h.texts(w));
    assert!(h.program().models.in_use.is_some());
    assert!(h.click_text(w, "Say Hello"));
    assert!(h.program().first_run.is_none());
    let _ = Decision::Allow;
}

#[test]
fn a_server_that_cant_be_reached_offers_retry_and_the_error_stays() {
    let mut h = harness("server-down");
    let w = h.window();
    let id = current(&h);
    let texts = h.texts(w);
    assert!(texts.iter().any(|t| t == "The model server can't be reached"), "{texts:?}");
    assert!(texts.iter().any(|t| t == "Connection refused (os error 111)"), "{texts:?}");
    assert!(texts.iter().any(|t| t == "Starting the model server…"), "it failed, so it isn't 'started': {texts:?}");
    assert_eq!(h.program().window_title(), "Is my graphics card running hot? — Assistant");
    assert!(h.click_text(w, "Retry"));
    for _ in 0..100 {
        h.advance(Duration::from_millis(100));
    }
    let c = h.program().conversation(id).unwrap();
    assert!(c.turn.is_none());
    let Some(Body::Answer(a)) = c.transcript.items.last().map(|i| &i.body) else { panic!() };
    assert!(a.text.starts_with("No: it's at **41 °C**"), "{}", a.text);
    let texts = h.texts(w);
    assert!(texts.iter().any(|t| t == "The model server can't be reached"), "what went wrong stays");
    assert!(!texts.iter().any(|t| t == "Retry"), "Retry only on the last thing said: {texts:?}");
}

#[test]
fn a_check_up_looks_at_a_dozen_things_and_reports_problems_first() {
    let mut h = harness("home");
    let w = h.window();
    let id = current(&h);
    h.send(Msg::Suggest(id, "Give my computer a check-up".into()));
    for _ in 0..400 {
        h.advance(Duration::from_millis(100));
    }
    let c = h.program().conversation(id).unwrap();
    assert!(c.turn.is_none());
    assert!(c.transcript.pending_approval().is_none(), "a check-up only looks");
    let texts = h.texts(w);
    assert!(texts.iter().any(|t| t.starts_with("Looked at 12 things")), "{texts:?}");
    let Some(Body::Answer(a)) = c.transcript.items.last().map(|i| &i.body) else { panic!() };
    let lines: Vec<&str> = a.text.lines().filter(|l| !l.is_empty()).collect();
    assert_eq!(lines[0], "Two things need attention.");
    assert!(lines[1].starts_with('✕') && lines[2].starts_with('⚠') && lines[3].starts_with('✓'));
    assert!(lines.iter().filter(|l| l.starts_with(char::is_numeric)).count() <= 3, "at most three fixes");
}
