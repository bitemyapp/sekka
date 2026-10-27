//! Alert actions must remain legible and operable as their window narrows.

use aizome_kit::prelude::*;
use aizome_kit::sidestep_ui::{Modifiers, NamedKey, Wrap};
use aizome_kit::testing::Harness;
use aizome_kit::text::{TextCache, TextSpec};
use aizome_kit::theme::FontSpec;

#[derive(Default)]
struct Prompt {
    window: Option<WindowKey>,
    labels: Vec<&'static str>,
    answers: Vec<usize>,
}

#[derive(Clone, Debug)]
enum Msg {
    Ask,
    Answer(usize),
}

impl Program for Prompt {
    type Msg = Msg;
    fn info(&self) -> AppInfo {
        AppInfo::new("org.aizome.AlertLayout", "Alert Layout", "Test")
    }
    fn launched(&mut self, cx: &mut Cx<Msg>) {
        self.window = Some(cx.open_window(WindowSpec::new("Alert").size(720.0, 640.0)));
    }
    fn update(&mut self, msg: Msg, cx: &mut Cx<Msg>) {
        match msg {
            Msg::Ask => {
                let mut alert = Alert::new("Choose an action", "Read the complete action before answering.")
                    .cancel_with("Cancel", Msg::Answer(0));
                for (i, label) in self.labels.iter().enumerate() {
                    alert = if i + 1 == self.labels.len() {
                        alert.default_button(*label, Msg::Answer(i + 1))
                    } else {
                        alert.destructive_button(*label, Msg::Answer(i + 1))
                    };
                }
                cx.alert(self.window.unwrap(), alert);
            }
            Msg::Answer(i) => self.answers.push(i),
        }
    }
    fn view(&self, _: WindowKey) -> Element<Msg> {
        text("Behind the alert").into()
    }
}

fn prompt(labels: Vec<&'static str>) -> (Harness<Prompt>, WindowKey) {
    let mut h = Harness::new(Prompt { labels, ..Prompt::default() });
    let w = h.window();
    h.focus_window(w, true);
    h.send(Msg::Ask);
    (h, w)
}

fn complete_labels(h: &Harness<Prompt>, w: WindowKey) {
    let mut cache = TextCache::default();
    for (index, label) in std::iter::once(&"Cancel").chain(h.program().labels.iter()).enumerate() {
        let rect = h.find(w, label).unwrap();
        assert!(rect.width() > 24.0, "no room for {label}: {rect:?}");
        let mut font = FontSpec::ui(h.theme().metrics.body);
        font.weight = if index == h.program().labels.len() { 600.0 } else { 500.0 };
        let mut spec = TextSpec::new(font, h.theme().text());
        spec.wrap = Wrap::Words;
        let text = cache.layout(h.theme(), label, &spec, Some(rect.width() - 24.0));
        assert!(
            rect.height() >= text.size().height.ceil() + 8.0,
            "{label} needs complete wrapped text height {:?}, got {rect:?}",
            text.size()
        );
        assert!(rect.x0 >= 0.0 && rect.x1 <= h.size(w).width, "{label} outside window: {rect:?}");
    }
}

#[test]
fn four_actions_keep_complete_labels() {
    let (mut h, w) = prompt(vec!["This and Later Events", "All Events", "Only This Event"]);
    complete_labels(&h, w);
    let first = h.find(w, "Cancel").unwrap();
    let last = h.find(w, "Only This Event").unwrap();
    assert!(last.y0 > first.y1, "overflowing actions should stack");
    h.save_png(w, std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("alert-four-actions.png"), 1.0);
    h.click_text(w, "Only This Event");
    assert_eq!(h.program().answers, [3]);
}

#[test]
fn long_labels_wrap_even_with_only_two_actions_and_after_resize() {
    let (mut h, w) = prompt(vec!["Permanently delete this occurrence and preserve every other event"]);
    for width in [720.0, 360.0, 240.0, 720.0] {
        h.resize(w, Size::new(width, 640.0));
        complete_labels(&h, w);
    }
    h.resize(w, Size::new(240.0, 640.0));
    h.save_png(w, std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("alert-long-action.png"), 1.0);
    h.named(w, NamedKey::Enter, Modifiers::NONE);
    assert_eq!(h.program().answers, [1]);
}

#[test]
fn compact_actions_stay_in_a_row_until_the_window_needs_a_stack() {
    let (mut h, w) = prompt(vec!["Delete", "Keep"]);
    assert_eq!(h.find(w, "Cancel").unwrap().y0, h.find(w, "Keep").unwrap().y0);
    h.resize(w, Size::new(240.0, 640.0));
    complete_labels(&h, w);
    assert!(h.find(w, "Keep").unwrap().y0 > h.find(w, "Cancel").unwrap().y1);
    for _ in 0..7 {
        h.named(w, NamedKey::Tab, Modifiers::NONE);
        let focus = h.focused_rect(w).unwrap();
        assert!(["Cancel", "Delete", "Keep"].iter().any(|label| h.find(w, label).unwrap() == focus));
    }
    h.named(w, NamedKey::Escape, Modifiers::NONE);
    assert_eq!(h.program().answers, [0]);
}

#[test]
fn short_windows_scroll_actions_into_view_for_keyboard_and_pointer() {
    let (mut h, w) = prompt(vec!["This and Later Events", "All Events", "Only This Event"]);
    h.resize(w, Size::new(240.0, 180.0));
    for _ in 0..4 {
        h.named(w, NamedKey::Tab, Modifiers::NONE);
        let focus = h.focused_rect(w).unwrap();
        assert!(focus.y0 >= 0.0 && focus.y1 <= 180.0, "focused action must be revealed: {focus:?}");
    }
    let last = h.find(w, "Only This Event").unwrap();
    assert!(last.y1 <= 180.0);
    h.click(w, last.center());
    assert_eq!(h.program().answers, [3]);
}
