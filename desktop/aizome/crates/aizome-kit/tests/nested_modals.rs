//! An alert over an editor must own input until it closes.

use aizome_kit::prelude::*;
use aizome_kit::sidestep_ui::{Modifiers, NamedKey};
use aizome_kit::testing::Harness;

#[derive(Default)]
struct Editor {
    window: Option<WindowKey>,
    sheet: Option<WindowKey>,
    main_text: String,
    draft: String,
    answers: Vec<&'static str>,
}

#[derive(Clone, Debug)]
enum Msg {
    Edit,
    Close,
    Ask,
    Answer(&'static str),
    MainText(String),
    Draft(String),
}

impl Program for Editor {
    type Msg = Msg;

    fn info(&self) -> AppInfo {
        AppInfo::new("org.aizome.ModalTest", "Modal Test", "Test")
    }

    fn launched(&mut self, cx: &mut Cx<Msg>) {
        self.window = Some(cx.open_window(WindowSpec::new("Editor").size(720.0, 520.0)));
        cx.focus("main-text");
    }

    fn update(&mut self, msg: Msg, cx: &mut Cx<Msg>) {
        match msg {
            Msg::Edit => {
                self.sheet = Some(cx.show_sheet(self.window.unwrap()));
                cx.focus("draft");
            }
            Msg::Close => {
                cx.close_window(self.sheet.take().unwrap());
            }
            Msg::Ask => cx.alert(
                self.window.unwrap(),
                Alert::new("Save this occurrence?", "The editor must not receive input behind this alert.")
                    .default_button("Save", Msg::Answer("save"))
                    .button("All Events", Msg::Answer("all"))
                    .cancel_button("Cancel"),
            ),
            Msg::Answer(answer) => self.answers.push(answer),
            Msg::MainText(text) => self.main_text = text,
            Msg::Draft(text) => self.draft = text,
        }
    }

    fn view(&self, window: WindowKey) -> Element<Msg> {
        if Some(window) == self.sheet {
            return aizome_kit::column![
                text("Event editor"),
                text_field(&self.draft).on_change(Msg::Draft).id("draft"),
                button("Confirm").default().on_press(Msg::Ask).id("confirm"),
                button("Close Editor").cancel().on_press(Msg::Close).id("close-editor")
            ]
            .spacing(12.0)
            .padding(16.0)
            .width(500.0)
            .into();
        }
        aizome_kit::column![
            text_field(&self.main_text).on_change(Msg::MainText).id("main-text"),
            button("Edit").on_press(Msg::Edit).id("edit")
        ]
        .spacing(12.0)
        .padding(16.0)
        .into()
    }
}

fn editor() -> (Harness<Editor>, WindowKey) {
    let mut h = Harness::new(Editor::default());
    let w = h.window();
    h.focus_window(w, true);
    h.type_text(w, "main");
    h.send(Msg::Edit);
    h.type_text(w, "draft");
    assert_eq!(h.program().draft, "draft");
    (h, w)
}

#[test]
fn top_alert_receives_pointer_and_blocks_editor() {
    let (mut h, w) = editor();
    let close = h.find_id(w, "close-editor").unwrap();
    h.send(Msg::Ask);
    h.click(w, close.center());
    assert!(h.program().sheet.is_some(), "the obscured editor must not close");
    assert!(h.click_text(w, "All Events"));
    assert_eq!(h.program().answers, ["all"]);
    assert!(h.program().sheet.is_some());
    assert!(!h.texts(w).iter().any(|s| s == "Save this occurrence?"));
}

#[test]
fn top_alert_owns_default_cancel_and_tab() {
    let (mut h, w) = editor();
    h.send(Msg::Ask);
    h.named(w, NamedKey::Enter, Modifiers::NONE);
    assert_eq!(h.program().answers, ["save"], "Return must activate the alert, not Confirm behind it");
    h.send(Msg::Ask);
    for modifiers in [Modifiers::NONE, Modifiers::SHIFT] {
        for _ in 0..7 {
            h.named(w, NamedKey::Tab, modifiers);
            let focus = h.focused_rect(w).unwrap();
            assert!(
                ["Cancel", "All Events", "Save"].iter().any(|label| h.find(w, label).unwrap().contains(focus.center())),
                "Tab escaped the alert: {focus:?}"
            );
        }
    }
    h.named(w, NamedKey::Escape, Modifiers::NONE);
    assert!(h.program().sheet.is_some(), "Escape must cancel the alert, not Close Editor");
    assert_eq!(h.program().answers, ["save"]);
    assert!(!h.texts(w).iter().any(|s| s == "Save this occurrence?"));
}

#[test]
fn covered_focus_cannot_receive_text_or_ime_and_is_restored() {
    let (mut h, w) = editor();
    assert_eq!(h.focused_name(w).as_deref(), Some("draft"));
    h.send(Msg::Ask);
    h.type_text(w, "leaked");
    h.ime(w, Some("한글"), "");
    h.ime(w, None, "preedit");
    assert_eq!(h.program().draft, "draft", "text and IME must not reach the covered editor");
    assert_ne!(h.focused_name(w).as_deref(), Some("draft"));
    h.named(w, NamedKey::Escape, Modifiers::NONE);
    assert_eq!(h.focused_name(w).as_deref(), Some("draft"));
    h.type_text(w, " resumed");
    assert_eq!(h.program().draft, "draft resumed");
    h.named(w, NamedKey::Escape, Modifiers::NONE);
    assert!(h.program().sheet.is_none());
    assert_eq!(h.focused_name(w).as_deref(), Some("main-text"));
    h.type_text(w, " resumed");
    assert_eq!(h.program().main_text, "main resumed");
}

#[test]
fn stacked_alerts_restore_only_the_uncovered_layer() {
    let (mut h, w) = editor();
    h.send(Msg::Ask);
    h.named(w, NamedKey::Tab, Modifiers::NONE);
    let first_focus = h.focused_rect(w).unwrap();
    h.send(Msg::Ask);
    h.named(w, NamedKey::Enter, Modifiers::NONE);
    assert_eq!(h.program().answers, ["save"]);
    assert_eq!(h.focused_rect(w), Some(first_focus));
    assert_ne!(h.focused_name(w).as_deref(), Some("draft"));
    h.named(w, NamedKey::Escape, Modifiers::NONE);
    assert_eq!(h.focused_name(w).as_deref(), Some("draft"));
    assert!(h.program().sheet.is_some());
}
