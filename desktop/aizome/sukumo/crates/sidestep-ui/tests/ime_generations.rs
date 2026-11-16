//! Late input crosses real window/prompt boundaries through the null
//! render thread, rather than bypassing them with widget-level injection.

fn main() {
    #[cfg(not(target_vendor = "apple"))]
    linux::run();
}

#[cfg(not(target_vendor = "apple"))]
mod linux {
    use sidestep_ui::testing::{self, PendingIme};
    use sidestep_ui::*;
    use std::time::Duration;

    #[derive(Clone, Copy, Debug)]
    enum Change {
        Reveal,
        Hide,
        Reset,
        ResetTwice,
        Caret,
        DuringCommit,
    }

    struct Test {
        change: Change,
        window: Option<WindowId>,
        pending: Option<PendingIme>,
        phase: usize,
        heard: Vec<Ime>,
        deadline: Option<TimerId>,
    }

    impl Handler for Test {
        fn launched(&mut self, cx: &mut Cx) {
            let w = cx.open_window(WindowOptions::new("IME generation regression").size(160.0, 80.0));
            self.window = Some(w);
            self.deadline = Some(cx.set_timer(Duration::from_secs(10)));
            let purpose = match self.change {
                Change::Hide => TextPurpose::Normal,
                _ => TextPurpose::Password,
            };
            cx.window(w).unwrap().set_text_input(true, purpose, None);
            self.pending = Some(testing::pending_ime(cx, w, Some("old-secret"), "old-preedit"));
            cx.set_timer(Duration::ZERO);
        }

        fn draw(&mut self, _: &mut Cx, _: WindowId, _: &mut Canvas) {}

        fn timer(&mut self, cx: &mut Cx, timer: TimerId) {
            assert_ne!(Some(timer), self.deadline, "{:?}: input never arrived", self.change);
            let w = self.window.unwrap();
            match self.phase {
                0 => {
                    let mut window = cx.window(w).unwrap();
                    match self.change {
                        Change::Reveal => window.set_text_input(true, TextPurpose::Normal, None),
                        Change::Hide => window.set_text_input(true, TextPurpose::Password, None),
                        Change::Reset | Change::DuringCommit => window.reset_text_input(),
                        Change::ResetTwice => {
                            window.reset_text_input();
                            self.pending = Some(testing::pending_ime(cx, w, Some("old-secret"), "old-preedit"));
                            cx.window(w).unwrap().reset_text_input();
                        }
                        Change::Caret => window.set_text_input(
                            true,
                            TextPurpose::Password,
                            Some(kurbo::Rect::new(1.0, 1.0, 2.0, 12.0)),
                        ),
                    }
                    self.phase = 1;
                    cx.set_timer(Duration::ZERO);
                }
                1 => {
                    // Another timer has run since the context change: an
                    // arrival/reset timer must not make this old input current.
                    self.pending.take().unwrap().deliver(cx);
                    testing::inject_ime(cx, w, Some("fresh"), "fresh-preedit");
                    self.phase = 2;
                }
                2 => {
                    assert!(matches!(self.change, Change::DuringCommit));
                    assert_eq!(self.heard, [Ime::Commit("fresh".into())]);
                    cx.quit();
                }
                _ => unreachable!(),
            }
        }

        fn window_event(&mut self, cx: &mut Cx, w: WindowId, event: WindowEvent) {
            if let WindowEvent::Ime(ime) = event {
                self.heard.push(ime.clone());
                if matches!(self.change, Change::DuringCommit) && ime == Ime::Commit("fresh".into()) {
                    cx.window(w).unwrap().reset_text_input();
                    cx.set_timer(Duration::ZERO);
                } else if matches!(&ime, Ime::Preedit { text, .. } if text == "fresh-preedit") {
                    cx.quit();
                }
            }
        }
    }

    pub fn run() {
        testing::use_null_backend();
        for change in
            [Change::Reveal, Change::Hide, Change::Reset, Change::ResetTwice, Change::Caret, Change::DuringCommit]
        {
            let test = Test { change, window: None, pending: None, phase: 0, heard: Vec::new(), deadline: None };
            let test = App::new().run(test).expect("null render thread");
            let mut expected = Vec::new();
            if matches!(change, Change::Caret) {
                expected.extend([
                    Ime::Commit("old-secret".into()),
                    Ime::Preedit { text: "old-preedit".into(), selection: Some(11..11) },
                ]);
            }
            expected.push(Ime::Commit("fresh".into()));
            if !matches!(change, Change::DuringCommit) {
                expected.push(Ime::Preedit { text: "fresh-preedit".into(), selection: Some(13..13) });
            }
            assert_eq!(test.heard, expected, "{change:?}");
        }
        println!("ime_generations: ok (six context and delivery orders)");
    }
}
